// Windows 专用:桌面宠物(main 窗口)的可交互区域鼠标穿透。
// 思路与 macOS 的 macos_pet.rs(NSTimer + ignoresMouseEvents)对齐,但 Windows 用
// SetWindowRgn 把窗口裁成「仅宠物 + 气泡矩形可交互」,区域外点击穿透到下层桌面/窗口。
//
// 本文件在所有平台都参与编译(不能用 #![cfg(windows)] 包整个文件,否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号)。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部,非 Windows 平台提供 no-op 实现。
//
// 注意 windows-sys 0.52 的类型约定:HWND / HRGN 等都是 isize 的 newtype(无 .is_invalid()
// 方法),判空用 `== 0`;SetWindowRgn 第三参数 bRedraw 是 BOOL(i32),传 1 而非 true。
//
// GDI 对象所有权规则:SetWindowRgn 调用**成功**后,region 的所有权转移给系统,
// 之后不能再对它调用 DeleteObject(会导致 double-free / 未定义行为)。
// 但如果 SetWindowRgn **失败**(返回 0),region 仍归调用者所有,必须自己 DeleteObject,
// 否则泄漏 GDI 句柄。下面所有 SetWindowRgn 调用都据此检查返回值。
//
// DPI 缩放注意事项:GetDeviceCaps(hdc, LOGPIXELSX) 返回的是系统/主显示器 DPI,
// 不是"这个窗口当前所在屏幕"的 DPI——如果窗口被拖到跟主屏缩放比例不同的副屏上,
// 用 GetDeviceCaps 算出来的 scale 会是错的(命中矩形跟实际物理像素对不上)。
// 改用 GetDpiForWindow(hwnd),它是按窗口取值,会随窗口跨屏移动动态更新
// (前提是窗口声明了 Per-Monitor V2 DPI 感知,Tauri 默认会声明)。
// GetDeviceCaps 保留作为 GetDpiForWindow 返回 0 时的兜底路径。

use std::sync::Mutex;

// Manager 特性提供 app.get_webview_window(...),在所有平台都需要
use tauri::Manager;

// 可交互矩形(CSS 逻辑像素,相对窗口内容区/视口左上角):(x, y, w, h)
type Rect = (f64, f64, f64, f64);
static HIT_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 是否已初始化(前端上报过矩形)。未初始化时保持整窗可交互。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

#[cfg(target_os = "windows")]
const WINDOW_CORNER_RADIUS: i32 = 14; // 与气泡/设置窗圆角一致
#[cfg(target_os = "windows")]
const LOGPIXELSX: i32 = 88; // GDI 常量:每逻辑英寸像素数(X 方向),仅兜底路径用

/// 前端调用:更新可交互区域列表(宠物 + 气泡矩形)。
/// 存为静态,待 apply_pet_hit_rects 时裁切窗口。空数组表示尚未渲染出有效元素,
/// 此时把 RECTS_INITIALIZED 置回 false,避免「已初始化但矩形为空」导致整窗永久穿透。
#[tauri::command]
pub fn set_pet_hit_rects(rects: Vec<(f64, f64, f64, f64)>) {
    let non_empty = !rects.is_empty();
    if let Ok(mut g) = HIT_RECTS.lock() {
        *g = rects;
    }
    if let Ok(mut init) = RECTS_INITIALIZED.lock() {
        *init = non_empty;
    }
}

/// 前端显式触发:把当前 hit rects 应用到 main 窗口(SetWindowRgn 即时生效)。
/// 非 Windows 平台为 no-op(macOS 走 macos_pet 的 NSTimer 方案)。
#[tauri::command]
pub fn apply_pet_hit_rects(app: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Some(w) = app.get_webview_window("main") {
            if let Ok(hwnd) = w.hwnd() {
                // tauri 的 HWND 是 windows_sys::Win32::Foundation::HWND(isize newtype)
                apply_hit_rects(hwnd.0 as isize);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

/// 隐藏 main 宠物窗口。
///
/// 修复要点(原代码顺序反了):原来是「先 SetWindowRgn(0) 清 region → 再 hide」,
/// 但 SetWindowRgn 会同步触发 DWM 立即按"整窗矩形、无裁切"重绘一帧,这一帧发生在
/// hide() 真正让窗口消失之前,于是用户会看到一闪而过的窗口边框/阴影。
///
/// 正确顺序:先 hide()(窗口已不可见,不会有画面暴露给用户),再清 region
/// (清掉裁切,避免下次 show 时残留旧的小 region 导致"缩略图"问题)。
/// 且清 region 这一步用 bRedraw=0(不重绘),因为窗口已经隐藏,不需要立即生效的重绘。
#[tauri::command]
pub fn hide_pet_window(app: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Some(w) = app.get_webview_window("main") {
            // 1. 先隐藏:窗口不可见后,任何后续重绘都不会呈现给用户
            let _ = w.hide();

            // 2. 再清 region(HRGN=0 表示清除裁切),bRedraw=0 不强制重绘
            if let Ok(hwnd) = w.hwnd() {
                use windows_sys::Win32::Graphics::Gdi::SetWindowRgn;
                unsafe {
                    SetWindowRgn(hwnd.0 as isize, 0, 0);
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
    }
}

/// 对外入口:main 窗口创建后调用,安装 Windows 穿透。
/// 非 Windows 平台为 no-op(macOS 由 macos_pet::setup_notify_interactive 处理)。
#[cfg(target_os = "windows")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(hwnd) = w.hwnd() {
            // 初始先整窗可交互(未初始化前不裁切,避免误穿透)
            apply_hit_rects(hwnd.0 as isize);
        }
    }
}

/// 给指定窗口设置**精确像素**的系统级圆角(Windows,方案二)。
///
/// 透明无边框窗口在 Windows 上仅靠 CSS `border-radius` 无法真正裁切窗口边角，
/// WebView2 的透明区域仍保持矩形，因此会出现「外框是直角」的问题。
///
/// 为什么不用 DWM 档位：
///   `DWMWA_WINDOW_CORNER_PREFERENCE` 只能选系统预设档位(小/大)，无法精确指定像素。
///   实测 Windows 11 的「大圆角」档实际只有 ~8px 左右，而本项目 CSS 的
///   `--radius-window` 是 14px，两者对不上 → 窗口边角裁得太小、与内容圆角错位。
///
/// 改用 `SetWindowRgn` + `CreateRoundRectRgn` 自绘整窗圆角矩形，半径取
/// `WINDOW_CORNER_RADIUS (14) × DPI 缩放`，与 CSS 14px 在任意缩放比例(2K/4K 高 DPI)
/// 下都物理对齐。
///
/// 注意：DWM 档位与 SetWindowRgn 不能共存(前者会覆盖后者视觉效果)，本函数只用 Rgn。
/// 另外窗口 resize 后 Rgn 不会自动跟随，调用方需在窗口尺寸变化时重新调用本函数。
#[cfg(target_os = "windows")]
pub fn setup_window_rounded_corners(hwnd: isize) {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateRoundRectRgn, DeleteObject, GetWindowRect, SetWindowRgn,
    };
    use windows_sys::Win32::Foundation::RECT;

    if hwnd == 0 {
        return;
    }

    // 取窗口实际像素矩形
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return;
    }
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    if w <= 0 || h <= 0 {
        return;
    }

    // 圆角半径 = 14px × DPI 缩放(逻辑像素→物理像素)，与 CSS --radius-window 对齐
    let scale = window_dpi_scale(hwnd);
    let radius = (WINDOW_CORNER_RADIUS as f64 * scale).round() as i32;
    // 半径不能超过短边一半，否则 CreateRoundRectRgn 行为异常
    let radius = radius.min(w / 2).min(h / 2).max(0);

    unsafe {
        let rgn = CreateRoundRectRgn(rect.left, rect.top, rect.right, rect.bottom, radius, radius);
        if rgn == 0 {
            return;
        }
        // SetWindowRgn 成功 → 所有权转移给系统，不可再 DeleteObject
        // 失败(返回 0) → 仍归我们所有，必须释放，避免 GDI 句柄泄漏
        let result = SetWindowRgn(hwnd, rgn, 1);
        if result == 0 {
            let _ = DeleteObject(rgn);
        }
    }
}

/// 给指定窗口设置精确像素圆角(非 Windows 平台 no-op)。
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn setup_window_rounded_corners(_hwnd: isize) {}

/// 计算窗口当前所在屏幕的 DPI 缩放系数。
///
/// 优先用 GetDpiForWindow(hwnd)——按窗口取值,窗口跨屏移动到不同缩放比例的
/// 显示器上时会动态更新,这是 Windows 10 1607+ 推荐的正确做法。
/// 仅当它返回 0(极少见,比如窗口句柄尚未完全与桌面窗口管理器关联)时,
/// 才退回旧的 GetDeviceCaps 方式兜底——注意这个兜底值取的是主屏/系统 DPI,
/// 窗口在副屏上时可能不准,但总比直接崩溃或用一个硬编码的 1.0 更合理。
#[cfg(target_os = "windows")]
fn window_dpi_scale(hwnd: isize) -> f64 {
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;

    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi > 0 {
        return dpi as f64 / 96.0;
    }

    // 兜底路径
    use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC};
    unsafe {
        let hdc = GetDC(hwnd);
        if hdc == 0 {
            1.0
        } else {
            let d = GetDeviceCaps(hdc, LOGPIXELSX);
            let _ = ReleaseDC(hwnd, hdc);
            if d <= 0 {
                1.0
            } else {
                d as f64 / 96.0
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_hit_rects(hwnd: isize) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
    };

    // hwnd 为 0 视为无效
    if hwnd == 0 {
        return false;
    }

    // 按窗口当前所在屏幕取 DPI scale(而不是固定用主屏/系统 DPI)
    let scale = window_dpi_scale(hwnd);

    let rects = match HIT_RECTS.lock() {
        Ok(g) => g.clone(),
        Err(_) => Vec::new(),
    };
    let initialized = matches!(RECTS_INITIALIZED.lock(), Ok(g) if *g);

    // 未初始化或空矩形 → 整窗可交互(传 0 region 清除裁切)
    if !initialized || rects.is_empty() {
        unsafe {
            SetWindowRgn(hwnd, 0, 1);
        }
        return true;
    }

    // 为每个矩形生成带圆角的 HRGN 并合并(RGN_OR = 并集)。
    // HRGN 在 windows-sys 0.52 即 isize;0 表示空。
    let mut combined: isize = 0;
    let mut ok = false;
    for &(x, y, w, h) in &rects {
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let l = (x * scale).round() as i32;
        let t = (y * scale).round() as i32;
        let r = ((x + w) * scale).round() as i32;
        let b = ((y + h) * scale).round() as i32;
        let radius = (WINDOW_CORNER_RADIUS as f64 * scale).round() as i32;
        let rgn = unsafe { CreateRoundRectRgn(l, t, r, b, radius, radius) };
        if rgn == 0 {
            continue;
        }
        if combined == 0 {
            combined = rgn;
            ok = true;
        } else {
            unsafe {
                CombineRgn(combined, combined, rgn, RGN_OR);
                // 子区域合并后不再需要,销毁以释放 GDI 对象
                DeleteObject(rgn);
            }
        }
    }

    if ok && combined != 0 {
        // 检查 SetWindowRgn 返回值。
        // 成功(非 0):region 所有权转移给系统,不能再 DeleteObject。
        // 失败(0):region 仍归我们所有,必须自己 DeleteObject,否则泄漏 GDI 句柄。
        let result = unsafe { SetWindowRgn(hwnd, combined, 1) };
        if result == 0 {
            unsafe {
                DeleteObject(combined);
            }
            // 应用失败,回退为整窗可交互,避免用户完全无法交互
            unsafe {
                SetWindowRgn(hwnd, 0, 1);
            }
        }
        true
    } else {
        // 没有有效矩形:整窗可交互
        unsafe {
            SetWindowRgn(hwnd, 0, 1);
        }
        true
    }
}