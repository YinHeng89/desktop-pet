// Windows 专用：桌面宠物(main 窗口)的可交互区域鼠标穿透。
//
// 方案：后端 Rust 线程轮询 + `WebviewWindow::set_ignore_cursor_events` 动态切换整窗穿透。
// 与 macOS 端「NSTimer 轮询 + setIgnoresMouseEvents」完全对等。
//
// 为什么不用 SetWindowRgn：
//   SetWindowRgn 会同时改变窗口「可见形状」与「命中区域」，而 WebView2 的内容由
//   msedgewebview2.exe 在独立渲染进程里异步合成并提交给 DWM。region 生效那一刻与
//   Chromium 那一帧的提交没有同步点，于是 DWM 会按「新 region × 旧帧」合成，表现为
//   多实例时随机被切掉下半身。这是两个系统之间缺少同步机制的架构性竞态，调矩形/DPI/
//   加日志都无法根治。
//
// 为什么不直接对 WebView2 子窗口做 WM_NCHITTEST 子类化：
//   WebView2 真正接收输入的 HWND（Chrome_WidgetWin_*）由别的进程创建，跨进程子类化
//   要么失败要么无效，且返回 HTTRANSPARENT 会触发跨线程消息弹跳导致 CPU 打满。
//
// 因此这里采用「整窗级穿透 + 后端轮询命中判断」：鼠标落在宠物/气泡矩形内 → 关闭穿透
//   （可交互），否则开启穿透（点击落到下层桌面/窗口）。Tauri 的 set_ignore_cursor_events
//   内部已经正确帮你叠加 WS_EX_TRANSPARENT / WS_EX_LAYERED，比裸调 SetWindowLongPtr 可靠。
//
// 本文件在所有平台都参与编译(不能用 #![cfg(windows)] 包整个文件,否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号)。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部,非 Windows 平台提供 no-op 实现。

use tauri::Manager;

// 以下 import 仅 Windows 的穿透轮询使用,故统一标 #[cfg(target_os = "windows")],
// 避免 macOS 编译报 unused/unused_imports。
#[cfg(target_os = "windows")]
use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::thread;
#[cfg(target_os = "windows")]
use std::time::Duration;
#[cfg(target_os = "windows")]
use crate::geometry::Rect;
#[cfg(target_os = "windows")]
use crate::geometry::point_in_rects;
// 以下静态/常量/函数只在 Windows 的穿透轮询中使用;非 Windows 平台无调用方,
// 故统一标 #[cfg(target_os = "windows")],避免 macOS 编译报 dead_code。
#[cfg(target_os = "windows")]
static HIT_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 是否已初始化(前端上报过矩形)。未初始化时保持整窗可交互(不穿透)。
#[cfg(target_os = "windows")]
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);
// 上次已应用的「穿透」状态，避免每个 tick 都调用 set_ignore_cursor_events（减少开销）。
#[cfg(target_os = "windows")]
static LAST_IGNORE: Mutex<bool> = Mutex::new(false);

/// 轮询间隔。约 20ms（50fps），与 macOS 的 16ms 同量级。
/// 太小会增加 CPU；太大则鼠标从透明区划到宠物上时会有可感知的「穿透未恢复」延迟。
#[cfg(target_os = "windows")]
const POLL_INTERVAL_MS: u64 = 20;

/// 内部:存储可交互矩形(供跨平台统一命令 update_interactive_rects 调用)。
#[cfg(target_os = "windows")]
pub(crate) fn store_hit_rects(rects: &[Rect]) {
    let non_empty = !rects.is_empty();
    if let Ok(mut g) = HIT_RECTS.lock() {
        *g = rects.to_vec();
    }
    if let Ok(mut init) = RECTS_INITIALIZED.lock() {
        *init = non_empty;
    }
}

/// 隐藏 main 宠物窗口：直接隐藏即可。
///
/// 之前用 SetWindowRgn(0) 清裁切是为了「下次 show 不残留旧 region」,但本方案不再使用
/// region,窗口可见形状完全由 WebView2 的透明背景决定,所以隐藏只需要 hide()。
#[tauri::command]
pub fn hide_pet_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// 给**设置窗口**设置系统级圆角 + 系统级阴影(Windows, DWM 方案)。
///
/// 设置窗口是普通无边框卡片窗口，需要「圆角 + 悬浮投影」的精致外观。
/// 宠物窗口因需要整窗穿透(用自己的方案),与设置窗口互不干扰。
#[cfg(target_os = "windows")]
pub fn setup_window_rounded_corners(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    if hwnd == 0 {
        return;
    }

    // windows-sys v0.52 中 HWND 是 isize 的 type alias，可直接作为 Win32 函数参数。
    // DWMWINDOWATTRIBUTE 枚举值(硬编码避免不同 windows-sys 版本命名差异)：
    //   DWMWA_WINDOW_CORNER_PREFERENCE = 33
    //   DWMWA_SHADOW                  = 2   (开启/关闭系统阴影)
    // DWM_WINDOW_CORNER_PREFERENCE：
    //   DWMWCP_DEFAULT = 0 / DONOTROUND = 1 / ROUNDSMALL = 2 / ROUNDLARGE = 3
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_SHADOW: u32 = 2;
    const DWMWCP_ROUNDLARGE: i32 = 3;
    const DWM_SHADOW_ENABLE: i32 = 2; // 2 = 启用默认系统阴影

    unsafe {
        let corner = DWMWCP_ROUNDLARGE;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );

        let shadow = DWM_SHADOW_ENABLE;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SHADOW,
            &shadow as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// 给指定窗口设置精确像素圆角(非 Windows 平台 no-op)。
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn setup_window_rounded_corners(_hwnd: isize) {}

/// 计算窗口当前所在屏幕的 DPI 缩放系数。用于将前端上报的 CSS 逻辑矩形
/// 换算到物理像素,跟全局鼠标坐标(GetCursorPos,物理像素)做命中判断。
///
/// 优先用 GetDpiForWindow(hwnd)——按窗口取值,窗口跨屏移动到不同缩放比例的
/// 显示器上时会动态更新,这是 Windows 10 1607+ 推荐的正确做法。
/// 仅当它返回 0(极少见)时,才退回 GetDeviceCaps 兜底。
#[cfg(target_os = "windows")]
fn window_dpi_scale(hwnd: isize) -> f64 {
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;

    // windows-sys v0.52 中 HWND = isize，hwnd 可直接传给 Win32 函数。
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi > 0 {
        return dpi as f64 / 96.0;
    }

    use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC};
    const LOGPIXELSX: i32 = 88;
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

/// 读取一次当前鼠标是否在可交互区域内（CSS 坐标，左上原点）。
///
/// 这是轮询的核心：拿全局鼠标位置，换算到窗口内容区坐标系，跟上报的矩形比较。
#[cfg(target_os = "windows")]
fn compute_should_ignore(w: &tauri::WebviewWindow) -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    use windows_sys::Win32::Foundation::POINT;

    // 未初始化 → 保持可交互(不穿透),避免启动初期误穿透。
    let initialized = matches!(RECTS_INITIALIZED.lock(), Ok(g) if *g);
    if !initialized {
        return Some(false);
    }

    let rects = match HIT_RECTS.lock() {
        Ok(g) => g.clone(),
        Err(_) => return None,
    };
    if rects.is_empty() {
        return Some(false);
    }

    // 用窗口自身的 hwnd 取 DPI,保证「矩形换算」与「窗口实际所在屏」一致。
    // windows-sys v0.52 的 HWND 是 isize alias，hwnd() 返回的即是 isize。
    let hwnd = match w.hwnd() {
        Ok(h) => h,
        Err(_) => return None,
    };
    let scale = window_dpi_scale(hwnd);

    // 全局鼠标位置(物理像素,屏幕坐标)。
    let mut pt = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut pt) } == 0 {
        // 拿不到鼠标位置,保守保持当前状态。
        return None;
    }

    // 「窗口内容区」左上角的屏幕坐标(物理像素)。
    // Tauri 的 outer_position() 返回 PhysicalPosition,左上角是 outer 左上角;
    // 内容区 = outer 左上角 + 标题栏/边框。本项目窗口无边框且 transparent,
    // outer 与 inner 基本重合,这里直接用 outer_position 作为内容区原点。
    let pos = match w.outer_position() {
        Ok(p) => p,
        Err(_) => return None,
    };

    // 鼠标相对窗口内容区左上角的物理像素坐标。
    let local_x = (pt.x - pos.x) as f64;
    let local_y = (pt.y - pos.y) as f64;

    // 换算回 CSS 逻辑像素(前端矩形的坐标系),再判定命中。
    let css_x = local_x / scale;
    let css_y = local_y / scale;

    let over = point_in_rects(&rects, css_x, css_y);
    // 命中 → 不穿透(false);未命中 → 穿透(true)。
    Some(!over)
}

/// 对外入口:main 窗口创建后调用,启动穿透轮询线程。
/// 非 Windows 平台为 no-op(macOS 由 macos_pet::setup_notify_interactive 处理)。
#[cfg(target_os = "windows")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    thread::spawn(move || {
        loop {
            // 每次取最新窗口引用;窗口不存在(已关闭)时跳过本 tick。
            if let Some(w) = app_handle.get_webview_window("main") {
                if let Some(should_ignore) = compute_should_ignore(&w) {
                    // 仅在状态变化时调用,避免每个 tick 都触发样式重算。
                    let changed = match LAST_IGNORE.lock() {
                        Ok(mut last) => {
                            let changed = *last != should_ignore;
                            *last = should_ignore;
                            changed
                        }
                        Err(_) => true,
                    };
                    if changed {
                        let _ = w.set_ignore_cursor_events(should_ignore);
                    }
                }
            }
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
    });
}

/// 非 Windows 平台:dummy 实现,保证 generate_handler! 能拿到符号。
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn setup_notify_interactive(_app: &tauri::AppHandle) {}
