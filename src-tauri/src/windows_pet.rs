// Windows 专用：桌面宠物（main 窗口）的可交互区域鼠标穿透。
// 思路与 macOS 的 macos_pet.rs（NSTimer + ignoresMouseEvents）对齐，但 Windows 用
// SetWindowRgn 把窗口裁成「仅宠物 + 气泡矩形可交互」，区域外点击穿透到下层桌面/窗口。
//
// 本文件在所有平台都参与编译（不能用 #![cfg(windows)] 包整个文件，否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号）。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部，非 Windows 平台提供 no-op 实现。
//
// 注意 windows-sys 0.52 的类型约定：HWND / HRGN 等都是 isize 的 newtype（无 .is_invalid()
// 方法），判空用 `== 0`；SetWindowRgn 第三参数 bRedraw 是 BOOL（i32），传 1 而非 true。

use std::sync::Mutex;

// Manager 特性提供 app.get_webview_window(...)，在所有平台都需要
use tauri::Manager;

// 可交互矩形（CSS 逻辑像素，相对窗口内容区/视口左上角）：(x, y, w, h)
type Rect = (f64, f64, f64, f64);
static HIT_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 是否已初始化（前端上报过矩形）。未初始化时保持整窗可交互。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

#[cfg(target_os = "windows")]
const WINDOW_CORNER_RADIUS: i32 = 14; // 与气泡/设置窗圆角一致
#[cfg(target_os = "windows")]
const LOGPIXELSX: i32 = 88; // GDI 常量：每逻辑英寸像素数（X 方向）

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
                // tauri 的 HWND 是 windows_sys::Win32::Foundation::HWND（isize newtype）
                apply_hit_rects(hwnd.0 as isize);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

/// 隐藏 main 宠物窗口（hide 前先清空 SetWindowRgn，避免 hide 瞬间 DWM 按
/// 旧 region 渲染装饰/边框闪现）。非 Windows 平台走普通 hide。
#[tauri::command]
pub fn hide_pet_window(app: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Some(w) = app.get_webview_window("main") {
            if let Ok(hwnd) = w.hwnd() {
                // 先清 region（HRGN=0 表示清除裁切），再 hide。
                // 不清的话，hide 瞬间 Windows DWM 会按"宠物+气泡"小 region 渲染
                // 窗口缩略图/装饰，而原始窗口矩形（更大）暴露在透明区外，视觉上
                // 就是"看到了窗口边框"。
                use windows_sys::Win32::Graphics::Gdi::SetWindowRgn;
                unsafe {
                    SetWindowRgn(hwnd.0 as isize, 0, 1);
                }
            }
            let _ = w.hide();
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
    }
}

/// 对外入口：main 窗口创建后调用，安装 Windows 穿透。
/// 非 Windows 平台为 no-op（macOS 由 macos_pet::setup_notify_interactive 处理）。
#[cfg(target_os = "windows")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(hwnd) = w.hwnd() {
            // 初始先整窗可交互（未初始化前不裁切，避免误穿透）
            apply_hit_rects(hwnd.0 as isize);
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_hit_rects(hwnd: isize) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRoundRectRgn, DeleteObject, GetDC, GetDeviceCaps, ReleaseDC,
        SetWindowRgn, RGN_OR,
    };

    // hwnd 为 0 视为无效
    if hwnd == 0 {
        return false;
    }

    // 计算 DPI scale：用窗口 DC 的 LOGPIXELSX（默认 96 → scale 1.0）。
    // SetWindowRgn 接收物理像素，需要把 CSS 逻辑像素乘以该 scale。
    let scale = unsafe {
        let hdc = GetDC(hwnd);
        if hdc == 0 {
            1.0f64
        } else {
            let dpi = GetDeviceCaps(hdc, LOGPIXELSX);
            let _ = ReleaseDC(hwnd, hdc);
            if dpi <= 0 {
                1.0f64
            } else {
                dpi as f64 / 96.0
            }
        }
    };

    let rects = match HIT_RECTS.lock() {
        Ok(g) => g.clone(),
        Err(_) => Vec::new(),
    };
    let initialized = matches!(RECTS_INITIALIZED.lock(), Ok(g) if *g);

    // 未初始化或空矩形 → 整窗可交互（传 0 region 清除裁切）
    if !initialized || rects.is_empty() {
        unsafe {
            SetWindowRgn(hwnd, 0, 1);
        }
        return true;
    }

    // 为每个矩形生成带圆角的 HRGN 并合并（RGN_OR = 并集）。
    // HRGN 在 windows-sys 0.52 即 isize；0 表示空。
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
                // 子区域合并后不再需要，销毁以释放 GDI 对象
                DeleteObject(rgn);
            }
        }
    }

    if ok && combined != 0 {
        unsafe {
            SetWindowRgn(hwnd, combined, 1);
        }
        true
    } else {
        // 没有有效矩形：整窗可交互
        unsafe {
            SetWindowRgn(hwnd, 0, 1);
        }
        true
    }
}
