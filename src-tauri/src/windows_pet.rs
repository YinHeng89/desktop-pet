// Windows 专用:桌面宠物(main 窗口)的可交互区域鼠标穿透。
// 思路与 macOS 的 macos_pet.rs(NSTimer + ignoresMouseEvents)对齐,但 Windows 用
// WM_NCHITTEST 子类实现「宠物/气泡可交互、区域外点击穿透到桌面」,而窗口可见形状
// 完全由 WebView2 的逐像素 alpha 决定(窗口始终是一整块透明矩形,不再用 SetWindowRgn
// 裁切)。这样渲染完整性(Chromium 负责)与交互穿透(WM_NCHITTEST 负责)彻底解耦,
// 消除「SetWindowRgn 即时生效 vs Chromium 帧异步提交」的竞态(多实例并发时尤为明显)。
//
// 本文件在所有平台都参与编译(不能用 #![cfg(windows)] 包整个文件,否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号)。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部,非 Windows 平台提供 no-op 实现。
//
// 注意 windows-sys 0.52 的类型约定:HWND 等句柄是 isize(newtype),判空用 `== 0`。
//
// 本文件不再对 main 宠物窗口调用 SetWindowRgn(那正是「region 提前生效 vs Chromium
// 帧提交」竞态的根因)。命中测试改由 WM_NCHITTEST 子类在点击时实时读取 HIT_RECTS 完成,
// 窗口可见形状完全交给 WebView2 的逐像素 alpha。仅「设置窗口」仍用 DWM 圆角/阴影
// (见 setup_window_rounded_corners),与 main 窗口互不干扰。
//
// DPI 缩放:WM_NCHITTEST 的屏幕坐标需换算回窗口内容区 CSS 逻辑像素才能和 HIT_RECTS
// 对齐,这里用 GetDpiForWindow(hwnd) 取按窗口取值的 scale(随窗口跨屏移动动态更新,
// 前提是窗口声明了 Per-Monitor V2 DPI 感知,Tauri 默认会声明);返回 0 时退 GetDeviceCaps。

use std::sync::Mutex;

// Manager 特性提供 app.get_webview_window(...),在所有平台都需要
use tauri::Manager;

// Windows 专属 API(仅 Windows 编译,避免在其他平台拉入 windows-sys 符号)。
// 命中测试由 WM_NCHITTEST 子类完成,不再依赖 SetWindowRgn。
// SetWindowSubclass / DefSubclassProc 来自 comctl32,在 windows-sys 0.52 中位于
// Win32::UI::Shell 模块(非 Controls),需启用 Win32_UI_Shell 特性。
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetClientRect, HTCLIENT, HTTRANSPARENT, WM_NCHITTEST,
};
// ScreenToClient 在 windows-sys 0.52 中归在 Win32::Graphics::Gdi 模块(而非 WindowsAndMessaging)。
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LRESULT, LPARAM, POINT, RECT, WPARAM};

// 可交互矩形(CSS 逻辑像素,相对窗口内容区/视口左上角):(x, y, w, h)
// 复用跨平台共享类型,保证与 macOS 端语义一致。
use crate::geometry::Rect;
static HIT_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 是否已初始化(前端上报过矩形)。未初始化时保持整窗可交互。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

#[cfg(target_os = "windows")]
const LOGPIXELSX: i32 = 88; // GDI 常量:每逻辑英寸像素数(X 方向),仅兜底路径用

/// 内部:存储可交互矩形(供跨平台统一命令 update_interactive_rects 调用)。
pub(crate) fn store_hit_rects(rects: &[Rect]) {
    let non_empty = !rects.is_empty();
    if let Ok(mut g) = HIT_RECTS.lock() {
        *g = rects.to_vec();
    }
    if let Ok(mut init) = RECTS_INITIALIZED.lock() {
        *init = non_empty;
    }
}

/// 前端调用:更新可交互区域列表(宠物 + 气泡矩形)。
/// 存为静态,待 apply_pet_hit_rects 时裁切窗口。空数组表示尚未渲染出有效元素,
/// 此时把 RECTS_INITIALIZED 置回 false,避免「已初始化但矩形为空」导致整窗永久穿透。
///
/// 向后兼容:旧命令名。后续前端统一走 update_interactive_rects。
#[tauri::command]
pub fn set_pet_hit_rects(rects: Vec<Rect>) {
    store_hit_rects(&rects);
}

/// 前端显式触发:在当前架构下为 no-op。
/// 命中测试已由 WM_NCHITTEST 子类在点击时实时读取 HIT_RECTS 完成,
/// 不再需要 SetWindowRgn 这种「提前把区域一次性贴到窗口」的时机敏感操作
/// (这正是多实例竞态的根因)。保留此 command 仅为前端兼容,无需再调用。
#[tauri::command]
pub fn apply_pet_hit_rects(app: tauri::AppHandle) {
    let _ = app;
}

/// 隐藏 main 宠物窗口。
///
/// 新架构下 main 窗口不再使用 SetWindowRgn 裁切(可见形状由 WebView2 逐像素 alpha
/// 负责,命中测试由 WM_NCHITTEST 子类负责),因此隐藏只需直接 hide(),无需先清 region。
/// 直接 hide 即可,不会有旧代码担心的"残留小 region 导致缩略图"问题。
#[tauri::command]
pub fn hide_pet_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// 对外入口:main 窗口创建后调用,安装 Windows 穿透。
/// 非 Windows 平台为 no-op(macOS 由 macos_pet::setup_notify_interactive 处理)。
#[cfg(target_os = "windows")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(hwnd) = w.hwnd() {
            // 安装 WM_NCHITTEST 子类:窗口保持整块透明矩形(可见形状由 WebView2 的
            // 逐像素 alpha 决定,不再用 SetWindowRgn 裁切),命中测试改由子类在点击时
            // 实时读取 HIT_RECTS 完成——彻底消除「region 提前生效 vs Chromium 帧提交」
            // 的竞态,多实例并发时各窗口也独立稳定。
            install_hit_test_subclass(hwnd.0 as isize);
        }
    }
}

/// 给**设置窗口**设置系统级圆角 + 系统级阴影(Windows, DWM 方案)。
///
/// 设置窗口是普通无边框卡片窗口，需要「圆角 + 悬浮投影」的精致外观。
/// 若用 `SetWindowRgn` 自绘圆角，会裁掉整个窗口 shape 之外的像素 —— 包括系统投影，
/// 导致投影消失(之前踩过的坑)。因此设置窗口改用 DWM 方案：
///
/// 1. `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUNDLARGE`(系统最大圆角档，~8px)，
///    由 DWM 在系统层裁切窗口四角，且**不裁掉投影**(投影由 DWM 合成在 shape 之外)。
/// 2. `DWMWA_SHADOW = 2`(开启系统默认阴影)，让无边框窗口获得悬浮投影。
///
/// 这样圆角与 CSS `--radius-window`(已改为 8px)在视觉上一致，且投影由系统绘制、不被裁。
///
/// 注意：DWM 档位只能选系统预设(大圆角档实测约 8px，无法精确到任意像素)，这正是
/// 本项目把设置窗圆角定为 8px 而非 14px 的原因。宠物主窗口的命中测试穿透现在由
/// WM_NCHITTEST 子类负责(见 install_hit_test_subclass，不再用 SetWindowRgn 裁切)，
/// 与本函数互不影响。
#[cfg(target_os = "windows")]
pub fn setup_window_rounded_corners(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    if hwnd == 0 {
        return;
    }

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
        // 大圆角档：与 CSS --radius-window:6px 视觉对齐
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );

        let shadow = DWM_SHADOW_ENABLE;
        // 开启系统阴影(无边框窗口默认无投影，需显式开启)
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

/// 在 main 窗口(及其 WebView2 子窗口)上安装 WM_NCHITTEST 子类,实现「宠物/气泡可交互,
/// 区域外点击穿透到桌面」,且**不**使用 SetWindowRgn 裁切可见形状。
///
/// 为什么必须同时子类化 WebView2 子窗口:
/// WebView2 在 Tauri 窗口内创建一个子 HWND 承载 Chromium。鼠标位于子窗口上方时,
/// WM_NCHITTEST 会先发给子窗口,子窗口默认返回 HTCLIENT,导致「区域外点击」被吞进
/// WebView2 而非穿透到桌面。因此除父类(主窗口)外,还要找到 WebView2 子窗口一并子类化:
/// 子类对区域外返回 HTTRANSPARENT,该命中结果会向上冒泡到父类,父类同样返回
/// HTTRANSPARENT,最终 OS 把点击交给我们下方的窗口(桌面/其他程序)。
#[cfg(target_os = "windows")]
pub fn install_hit_test_subclass(main_hwnd: isize) {
    if main_hwnd == 0 {
        return;
    }
    unsafe {
        // 1) 主窗口自身
        let _ = SetWindowSubclass(main_hwnd, Some(hit_test_subclass), 1, main_hwnd as usize);

        // 2) 找到 WebView2 子窗口并同样子类化
        let mut collector = ChildCollector {
            matches: Vec::new(),
            all: Vec::new(),
        };
        EnumChildWindows(
            main_hwnd,
            Some(collect_child),
            &mut collector as *mut _ as LPARAM,
        );
        let child = if !collector.matches.is_empty() {
            collector.matches[0]
        } else if collector.all.len() == 1 {
            collector.all[0]
        } else {
            // 取客户端面积最大的子窗口(WebView2 通常占满主窗口)
            let mut best: HWND = 0;
            let mut best_area: i64 = 0;
            for &h in &collector.all {
                let mut r = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                if GetClientRect(h, &mut r) != 0 {
                    let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
                    if area > best_area {
                        best_area = area;
                        best = h;
                    }
                }
            }
            best
        };
        if child != 0 {
            let _ = SetWindowSubclass(child, Some(hit_test_subclass), 1, main_hwnd as usize);
        }
    }
}

/// WM_NCHITTEST 子类回调:区域外返回 HTTRANSPARENT(穿透),区域内返回 HTCLIENT(可交互)。
/// ref_data 传入主窗口 HWND(usize),用于把屏幕坐标换算成窗口内容区 CSS 逻辑坐标。
#[cfg(target_os = "windows")]
unsafe extern "system" fn hit_test_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    ref_data: usize,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        let main_hwnd = ref_data as isize;
        // lparam = 屏幕坐标(低 16 位 x,高 16 位 y)
        let sx = (lparam & 0xffff) as i16 as i32;
        let sy = ((lparam >> 16) & 0xffff) as i16 as i32;
        let mut pt = POINT { x: sx, y: sy };
        if main_hwnd != 0 && ScreenToClient(main_hwnd, &mut pt) != 0 {
            let scale = window_dpi_scale(main_hwnd);
            if scale > 0.0 {
                let css_x = pt.x as f64 / scale;
                let css_y = pt.y as f64 / scale;
                let (rects, initialized) = {
                    let r = HIT_RECTS
                        .lock()
                        .map(|g| g.clone())
                        .unwrap_or_default();
                    let i = RECTS_INITIALIZED.lock().map(|g| *g).unwrap_or(false);
                    (r, i)
                };
                // 未初始化或空矩形:整窗可交互(等效旧逻辑 region=0),避免完全无法交互
                if !initialized || rects.is_empty() {
                    return HTCLIENT as LRESULT;
                }
                if crate::geometry::point_in_rects(&rects, css_x, css_y) {
                    return HTCLIENT as LRESULT;
                }
                return HTTRANSPARENT as LRESULT;
            }
        }
        // 坐标转换失败兜底:整窗可交互
        return HTCLIENT as LRESULT;
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// EnumChildWindows 回调:收集所有子窗口,并标记类名匹配 WebView2/Chromium 的。
#[cfg(target_os = "windows")]
struct ChildCollector {
    matches: Vec<HWND>,
    all: Vec<HWND>,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn collect_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let col = &mut *(lparam as *mut ChildCollector);
    let mut buf = [0u16; 256];
    let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    let class = if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    };
    if class.contains("WebView2") || class.contains("Chrome_WidgetWin") {
        col.matches.push(hwnd);
    }
    col.all.push(hwnd);
    1
}

/// 对外入口:main 窗口创建后调用,安装平台穿透。
/// Windows:WM_NCHITTEST 子类做命中测试穿透(见 install_hit_test_subclass);
/// macOS:由 macos_pet 模块处理(见 setup_notify_interactive 分支);
/// Linux:暂未实现不规则窗口穿透(需要 X11 XShape/XFixes 或 Wayland layer-shell),
/// 此时仅打印一次友好提示,宠物窗口退化为普通置顶透明窗口(无局部穿透)。
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn setup_notify_interactive(_app: &tauri::AppHandle) {
    eprintln!(
        "[windows_pet] Linux 暂不支持不规则窗口穿透:宠物窗口将以普通置顶透明窗口显示,\
         点击宠物区域外也会命中窗口(无穿透)。如需完整支持,请参考 X11 XShape/XFixes 方案。"
    );
}