// 跨平台共享的纯几何/坐标逻辑。
//
// 设计目标:把「与操作系统无关」的计算从 macos_pet / windows_pet 中抽出来,
// 只写一份、两端共用,避免口径分裂(例如命中判定、rect×scale 换算、scale 范围 clamp)。
// 这里不含任何平台 API 调用,因此可以在任意平台编译并单测。
//
// 坐标约定(与前端一致):
// - Rect = (x, y, w, h),单位 CSS 逻辑像素,相对窗口内容区/视口左上角。
// - macOS 由于原点在左下,转换逻辑见 macos_pet.rs 的 to_css_coords,不在此处。

/// 可交互矩形(CSS 逻辑像素,相对窗口内容区左上角):(x, y, w, h)
pub type Rect = (f64, f64, f64, f64);

/// 缩放范围。必须与前端 src/store/pet.ts 的 MIN_SCALE / MAX_SCALE 保持一致。
/// 任何一处修改都应同步另一处,否则宠物会浮在窗口偏左上的位置。
pub const MIN_SCALE: f64 = 0.5;
pub const MAX_SCALE: f64 = 1.3;

/// 把 scale 夹到允许范围。纯函数,便于单测。
///
/// 语义与 `f64::clamp` 完全一致（含 NaN：比较恒为 false，原样返回 NaN）。
/// MIN_SCALE < MAX_SCALE 为常量，故 clamp 的 panic 前提（min > max / min 或 max 为 NaN）不成立。
pub fn clamp_scale(scale: f64) -> f64 {
    scale.clamp(MIN_SCALE, MAX_SCALE)
}

/// 判断点 (px, py) 是否落在任一矩形内(含边界)。
/// macOS 的 hit_interactive 与 Windows 的 rect 遍历原本各写一份,现统一为此函数,
/// 保证两端命中语义完全一致。
///
/// 矩形按 `(x, y, w, h)` 解释，`w`/`h` 可为负（表示矩形向左/向上延伸）；
/// 这里统一按真实包围盒 `[min(x, x+w), max(x, x+w)]` 判断，与符号无关。
pub fn point_in_rects(rects: &[Rect], px: f64, py: f64) -> bool {
    for &(x, y, w, h) in rects {
        let (x0, x1) = if w >= 0.0 { (x, x + w) } else { (x + w, x) };
        let (y0, y1) = if h >= 0.0 { (y, y + h) } else { (y + h, y) };
        if px >= x0 && px <= x1 && py >= y0 && py <= y1 {
            return true;
        }
    }
    false
}

/// 把一组 CSS 逻辑像素矩形换算成物理像素矩形(乘以 scale)。
/// Windows 端 SetWindowRgn 需要物理像素;macOS 的 NSTimer 方案直接用 CSS 坐标,
/// 不需要此换算(它在 ObjC 层用 frame 处理)。
///
/// 此处只做 `rect * scale`,DPI 由调用方决定是否额外乘(见 windows_pet::apply_hit_rects)。
/// 注意:此函数在 macOS 编译目标下无调用方(仅 Windows cfg 内使用),
/// 故加 #[allow(dead_code)] 抑制跨平台编译的未使用警告。
#[allow(dead_code)]
pub fn rects_to_logical_physical(rects: &[Rect], scale: f64) -> Vec<Rect> {
    rects
        .iter()
        .map(|&(x, y, w, h)| (x * scale, y * scale, w * scale, h * scale))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_scale_bounds() {
        assert_eq!(clamp_scale(0.1), MIN_SCALE);
        assert_eq!(clamp_scale(2.0), MAX_SCALE);
        assert_eq!(clamp_scale(1.0), 1.0);
        assert_eq!(clamp_scale(MIN_SCALE - 1e-9), MIN_SCALE);
        assert_eq!(clamp_scale(MAX_SCALE + 1e-9), MAX_SCALE);
    }

    #[test]
    fn clamp_scale_nan_passthrough() {
        // NaN 与任意值比较恒为 false，应原样返回（不 panic、不掉到边界）
        let r = clamp_scale(f64::NAN);
        assert!(r.is_nan());
        // inf 应被夹到边界
        assert_eq!(clamp_scale(f64::INFINITY), MAX_SCALE);
        assert_eq!(clamp_scale(f64::NEG_INFINITY), MIN_SCALE);
    }

    #[test]
    fn point_in_rects_basic() {
        let rects = vec![(0.0, 0.0, 100.0, 50.0), (200.0, 0.0, 50.0, 50.0)];
        assert!(point_in_rects(&rects, 10.0, 10.0));
        assert!(point_in_rects(&rects, 220.0, 40.0));
        // 间隙处不应命中
        assert!(!point_in_rects(&rects, 150.0, 10.0));
        // 边界应包含
        assert!(point_in_rects(&rects, 100.0, 50.0));
        // 空列表任何点都不命中
        assert!(!point_in_rects(&[], 0.0, 0.0));
    }

    #[test]
    fn point_in_rects_negative_dims_and_overlap() {
        // 负宽高矩形：取绝对值后仍应正确命中
        let rects = vec![(-10.0, -10.0, -20.0, -20.0)];
        assert!(point_in_rects(&rects, -15.0, -15.0));
        // 重叠矩形（两个矩形部分重叠）：点落在任一内即命中
        let overlap = vec![(0.0, 0.0, 30.0, 30.0), (20.0, 20.0, 30.0, 30.0)];
        assert!(point_in_rects(&overlap, 25.0, 25.0));
        assert!(!point_in_rects(&overlap, 100.0, 100.0));
    }

    #[test]
    fn rects_to_logical_physical_scales() {
        let rects = vec![(10.0, 20.0, 30.0, 40.0)];
        let out = rects_to_logical_physical(&rects, 2.0);
        assert_eq!(out[0], (20.0, 40.0, 60.0, 80.0));
        // scale=1 不变
        assert_eq!(rects_to_logical_physical(&rects, 1.0), rects);
        // scale=0 全部归零
        assert_eq!(
            rects_to_logical_physical(&rects, 0.0),
            vec![(0.0, 0.0, 0.0, 0.0)]
        );
    }
}
