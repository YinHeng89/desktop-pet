// Windows 专用：桌面宠物（main 窗口）的可交互区域鼠标穿透。
// 思路与 macOS 的 macos_pet.rs（NSTimer + ignoresMouseEvents）对齐，但 Windows 用
// SetWindowRgn 把窗口裁成「仅宠物 + 气泡矩形可交互」，区域外点击穿透到下层桌面/窗口。
//
// 本文件在所有平台都参与编译（不能用 #![cfg(windows)] 包整个文件，否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号）。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部，非 Windows 平台提供 no-op 实现。

use std::sync::Mutex;

// 仅在 Windows 平台需要 Manager（获取 HWND）与圆角常量
#[cfg(target_os = "windows")]
use tauri::Manager;

// 可交互矩形（CSS 逻辑像素，相对窗口内容区/视口左上角）：(x, y, w, h)
type Rect = (f64, f64, f64, f64);
static HIT_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 是否已初始化（前端上报过矩形）。未初始化时保持整窗可交互。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

#[cfg(target_os = "windows")]
const WINDOW_CORNER_RADIUS: i32 = 14; // 与气泡/设置窗圆角一致

/// 前端调用：更新可交互区域列表（宠物 + 气泡矩形）。
/// 存为静态，待 apply_pet_hit_rects 时裁切窗口。空数组表示尚未渲染出有效元素，
/// 此时把 RECTS_INITIALIZED 置回 false，避免「已初始化但矩形为空」导致整窗永久穿透。
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

/// 前端显式触发：把当前 hit rects 应用到 main 窗口（SetWindowRgn 即时生效）。
/// 非 Windows 平台为 no-op（macOS 走 macos_pet 的 NSTimer 方案）。
#[tauri::command]
pub fn apply_pet_hit_rects(app: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Some(w) = app.get_webview_window("main") {
            if let Ok(hwnd) = w.hwnd() {
                let ptr = hwnd.0 as *mut std::ffi::c_void;
                apply_hit_rects(ptr);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

/// 对外入口：main 窗口创建后调用，安装 Windows 穿透。
/// 非 Windows 平台为 no-op（macOS 由 macos_pet::setup_notify_interactive 处理）。
#[cfg(target_os = "windows")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(hwnd) = w.hwnd() {
            let ptr = hwnd.0 as *mut std::ffi::c_void;
            // 初始先整窗可交互（未初始化前不裁切，避免误穿透）
            apply_hit_rects(ptr);
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_hit_rects(hwnd: *mut std::ffi::c_void) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetDpiForWindow;
    use windows_sys::Win32::Foundation::HWND;

    let hwnd = hwnd as HWND;
    if hwnd.is_invalid() {
        return false;
    }

    // SetWindowRgn 使用物理像素，需要把 CSS 逻辑像素乘以 DPI scale
    let scale = unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            1.0f64
        } else {
            dpi as f64 / 96.0
        }
    };

    let rects = match HIT_RECTS.lock() {
        Ok(g) => g.clone(),
        Err(_) => Vec::new(),
    };
    let initialized = matches!(RECTS_INITIALIZED.lock(), Ok(g) if *g);

    // 未初始化或空矩形 → 整窗可交互（传 null region 清除裁切）
    if !initialized || rects.is_empty() {
        unsafe {
            SetWindowRgn(hwnd, std::ptr::null_mut(), true);
        }
        return true;
    }

    // 为每个矩形生成带圆角的 HRGN 并合并（RGN_OR = 并集）
    let mut combined: windows_sys::Win32::Graphics::Gdi::HRGN = std::ptr::null_mut();
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
        if rgn.is_invalid() {
            continue;
        }
        if combined.is_invalid() {
            combined = rgn;
            ok = true;
        } else {
            unsafe {
                CombineRgn(combined, combined, rgn, RGN_OR);
                // 子区域合并后不再需要，销毁以释放 GDI 对象
                DeleteObject(rgn as _);
            }
        }
    }

    if ok && !combined.is_invalid() {
        unsafe {
            SetWindowRgn(hwnd, combined, true);
        }
        true
    } else {
        // 没有有效矩形：整窗可交互
        unsafe {
            SetWindowRgn(hwnd, std::ptr::null_mut(), true);
        }
        true
    }
}
