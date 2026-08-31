// 宠物窗口布局的纯计算：尺寸推导 + 右下角锚点重定位。
//
// 本模块零平台依赖（不碰 tauri / 窗口句柄），所有函数 100% 可单测。
// 原逻辑来自 lib.rs 的 `pet_window_size` 与 `resize_pet_window` 内联计算，
// 抽纯后行为逐字节对齐（见本文件底部黄金值快照测试，RK4 防护）。

use crate::domain::geometry::clamp_scale;

/// 宠物帧逻辑宽（scale=1 时，与 domain::pet 的 FRAME_W 同源）。
pub const FRAME_W: f64 = 192.0;
/// 宠物帧逻辑高（scale=1 时）。
pub const FRAME_H: f64 = 208.0;
/// 气泡区逻辑高（scale=1 时）。
pub const BUBBLE_ZONE_H: f64 = 156.0;
/// 窗口基线宽：scale=1 时宠物宽 192 < 320，取 320 保证气泡不挤（与 worktrack 一致）。
pub const BASE_WINDOW_W: f64 = 320.0;
/// 窗口四周缓冲（落在左/上透明区），缩放瞬间宠物已先渲染_new scale，
/// 比旧窗口大一圈避免被 OS 裁掉闪一下。
pub const WINDOW_PAD: f64 = 24.0;
/// 宠物区底部留白（scale=1 时 16px）。
pub const WINDOW_BOTTOM_PAD: f64 = 16.0;

/// 按缩放比例计算 main 宠物窗口的逻辑尺寸（宽, 高）。
///
/// scale 会被 `clamp_scale` 夹到 [MIN_SCALE, MAX_SCALE]，保证 Rust 与前端口径一致。
/// 窗口 = 气泡区 + 宠物区 + 底部留白，再四周加 `WINDOW_PAD` 缓冲。
pub fn pet_window_size(scale: f64) -> (f64, f64) {
    let scale = clamp_scale(scale);
    let pet_w = (FRAME_W * scale).round();
    let pet_h = (FRAME_H * scale).round();
    let bubble_h = (BUBBLE_ZONE_H * scale).round();
    // 宽：基线 320 × scale（等比缩放）；高：气泡区 + 宠物区 + 底部留白
    let mut ww = pet_w.max(BASE_WINDOW_W * scale);
    let mut wh = bubble_h + pet_h + WINDOW_BOTTOM_PAD;
    // 缓冲：窗口比内容大一圈，避免缩放过程中宠物短暂超出旧窗口被 OS 裁掉而闪。
    ww += WINDOW_PAD;
    wh += WINDOW_PAD;
    (ww, wh)
}

/// 以窗口【右下角】为锚点，根据旧位置/旧尺寸与新尺寸，计算新左上角（物理像素）。
///
/// 物理像素 = 逻辑像素 × scale_factor。`old_*` 是缩放前的物理坐标与尺寸；
/// 返回的新 (x, y) 让「旧右下角」与「新右下角」完全重合，宠物贴右下角原地缩放不漂移。
pub fn anchor_bottom_right(
    old_x: i32,
    old_y: i32,
    old_w: u32,
    old_h: u32,
    new_w: f64,
    new_h: f64,
    scale_factor: f64,
) -> (i32, i32) {
    // 旧右下角屏幕坐标（物理像素）：调用前的位置 + 调用前的尺寸
    let right = old_x as f64 + old_w as f64;
    let bottom = old_y as f64 + old_h as f64;
    // 先按「旧右下角」把新左上角定位回去，保证右下角不动
    let x = (right - new_w * scale_factor).round() as i32;
    let y = (bottom - new_h * scale_factor).round() as i32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── pet_window_size 黄金值快照 ──
    // 抽取前先在 lib.rs 用同一公式核算，抽取后断言不变（RK4：防浮点舍入漂移）。
    #[test]
    fn window_size_golden_scale_0_5() {
        assert_eq!(pet_window_size(0.5), (184.0, 222.0));
    }

    #[test]
    fn window_size_golden_scale_0_7() {
        assert_eq!(pet_window_size(0.7), (248.0, 295.0));
    }

    #[test]
    fn window_size_golden_scale_1_0() {
        assert_eq!(pet_window_size(1.0), (344.0, 404.0));
    }

    #[test]
    fn window_size_golden_scale_1_3() {
        assert_eq!(pet_window_size(1.3), (440.0, 513.0));
    }

    #[test]
    fn window_size_clamps_out_of_range() {
        // MIN_SCALE=0.5 / MAX_SCALE=1.3，越界被夹回边界
        assert_eq!(pet_window_size(0.1), pet_window_size(0.5));
        assert_eq!(pet_window_size(2.0), pet_window_size(1.3));
        assert_eq!(pet_window_size(f64::NEG_INFINITY), pet_window_size(0.5));
    }

    // ── anchor_bottom_right ──
    #[test]
    fn anchor_keeps_bottom_right_corner() {
        // 旧窗口：物理 (100,200) 尺寸 320×380，scale_factor=2.0；新逻辑尺寸 344×404
        let (x, y) = anchor_bottom_right(100, 200, 320, 380, 344.0, 404.0, 2.0);
        // 旧右下角 = (420, 580)；新左下应使其回到 (420, 580)
        assert_eq!((x, y), (-268, -228));
        let new_right = x as f64 + 344.0 * 2.0;
        let new_bottom = y as f64 + 404.0 * 2.0;
        assert_eq!((new_right as i32, new_bottom as i32), (420, 580));
    }

    #[test]
    fn anchor_no_drift_across_consecutive_resizes() {
        // 连续多次 resize：右下角必须始终钉在原处，不累积漂移。
        // 注意：窗口物理尺寸 = 逻辑尺寸 × scale_factor，下一轮 resize 的「旧尺寸」
        // 必须用物理尺寸参与锚点计算（与 resize_pet_window 的 outer_size 一致）。
        let sf = 2.0_f64;
        let (mut px, mut py) = (100_i32, 200_i32);
        let (mut pw, mut ph) = (320_u32, 380_u32);
        let orig_right = px as f64 + pw as f64;
        let orig_bottom = py as f64 + ph as f64;

        for (nw, nh) in [(344.0, 404.0), (248.0, 295.0), (440.0, 513.0)] {
            let (nx, ny) = anchor_bottom_right(px, py, pw, ph, nw, nh, sf);
            // 应用：新物理尺寸 = 新逻辑尺寸 × sf
            px = nx;
            py = ny;
            pw = (nw * sf).round() as u32;
            ph = (nh * sf).round() as u32;
            let right = px as f64 + pw as f64;
            let bottom = py as f64 + ph as f64;
            assert_eq!(
                (right as i32, bottom as i32),
                (orig_right as i32, orig_bottom as i32)
            );
        }
    }
}
