// Windows 专用:桌面宠物(main 窗口)的可交互区域鼠标穿透。
//
// 架构:完整矩形窗口 + 透明背景 + WM_NCHITTEST 命中测试穿透。
// 渲染(整窗矩形,WebView2 想画多大画多大)与交互穿透(命中测试)彻底解耦,
// 从设计上消除了「SetWindowRgn 与 WebView2 合成竞态导致的渲染不全/丢帧」问题,
// 也是唯一能保证「多开 N 个实例全部稳定正常」的方案。
//
// 关键点(为什么必须子类化 WebView2 子窗口):
// WebView2 会把内容渲染到父窗口下的子 HWND(Chrome_WidgetWin_0 等)。WM_NCHITTEST
// 消息默认会被光标下的**子窗口**接收并返回 HTCLIENT,导致父窗口 WndProc 根本收不到,
// 命中测试穿透因此失效(这正是「只子类化父窗口」方案失败的原因,见 WebView2Feedback
// #446)。故必须用 EnumChildWindows 枚举出 WebView2 渲染子窗口,并对其 SetWindowLongPtrW
// 子类化,统一挂同一个 WM_NCHITTEST 回调。
//
// 本文件在所有平台都参与编译(不能用 #![cfg(windows)] 包整个文件,否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号)。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部,非 Windows 平台提供 no-op 实现。

use std::sync::Mutex;

// Manager 特性提供 app.get_webview_window(...),在所有平台都需要
use tauri::Manager;

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
/// 存为静态,WM_NCHITTEST 回调里即时读取做命中测试,无需再触发任何"应用"步骤。
/// 空数组表示尚未渲染出有效元素,此时把 RECTS_INITIALIZED 置回 false,
/// 命中测试退化为整窗可交互。
///
/// 向后兼容:旧命令名。前端统一走 update_interactive_rects。
#[tauri::command]
pub fn set_pet_hit_rects(rects: Vec<Rect>) {
    store_hit_rects(&rects);
}

/// 前端显式触发:把当前 hit rects 应用到 main 窗口。
/// WM_NCHITTEST 方案下,命中测试回调每次鼠标移动都会实时读 HIT_RECTS,
/// 本命令退化为 no-op(保留仅为向后兼容旧前端调用)。
#[tauri::command]
pub fn apply_pet_hit_rects(app: tauri::AppHandle) {
    let _ = app;
}

/// 隐藏 main 宠物窗口。
/// WM_NCHITTEST 方案下窗口永远是完整矩形、不再用 SetWindowRgn 裁切,
/// 因此也无需「先清 region 再 hide」的时序处理,直接 hide 即可。
#[tauri::command]
pub fn hide_pet_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// 对外入口:main 窗口创建后调用,安装 Windows 穿透(子类化父窗口 + WebView2 子窗口)。
/// 非 Windows 平台为 no-op(macOS 由 macos_pet::setup_notify_interactive 处理)。
#[cfg(target_os = "windows")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(hwnd) = w.hwnd() {
            install_click_through(hwnd.0 as isize);
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
/// 本项目把设置窗圆角定为 8px 而非 14px 的原因。宠物窗口走 WM_NCHITTEST 命中测试穿透
/// (整窗矩形 + 透明背景，不再用 SetWindowRgn 裁切)，与本函数互不影响。
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
/// 用于把「CSS 逻辑像素的命中矩形」换算成物理像素后再与光标屏幕坐标比对。
/// 优先 GetDpiForWindow(按窗口取值,跨屏移动时动态正确),GetDeviceCaps 仅兜底。
#[cfg(target_os = "windows")]
fn window_dpi_scale(hwnd: isize) -> f64 {
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;

    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi > 0 {
        return dpi as f64 / 96.0;
    }

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

/// WM_NCHITTEST 子类化回调。
///
/// 对父窗口(Tauri Window)与其下枚举出的 WebView2 子窗口(Chrome_WidgetWin_* 等)
/// 统一生效。命中测试逻辑:
/// - 未初始化 / 无矩形 → 整窗可交互(HTCLIENT)。
/// - 光标落在任一命中矩形内(宠物/气泡)→ HTCLIENT(窗口自己吃,可拖拽/交互)。
/// - 否则 → HTTRANSPARENT(点击穿透到下层桌面/窗口)。
/// 其他消息一律 CallWindowProcW 转发原 WndProc。
///
/// 注意:此回调由系统在主线程的消息循环中调用,内部只做轻量读锁(HIT_RECTS 的
/// lock),持锁极短、不跨任何可能阻塞的调用,避免卡住消息循环。
#[cfg(target_os = "windows")]
unsafe extern "system" fn hit_test_subclass(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, HTCLIENT, HTTRANSPARENT, WM_NCHITTEST, WNDPROC,
    };

    // 取出原 WndProc(子类化时已存进 SUBCLASSED 表;此刻 GetWindowLongPtrW 返回的是
    // 我们自己的函数指针,不能再用它取原值)。
    let original: WNDPROC = match SUBCLASSED.lock() {
        Ok(g) => g.get(&hwnd).and_then(|e| e.original),
        Err(_) => None,
    };

    if msg == WM_NCHITTEST {
        let hit = point_in_hit_rects(hwnd, lparam);
        // HTCLIENT 是 u32、HTTRANSPARENT 是 i32,统一先转成 i32 再转 isize
        let result: i32 = if hit { HTCLIENT as i32 } else { HTTRANSPARENT };
        return result as isize;
    }

    // 其他消息转发原 WndProc;缺失时退化为默认处理
    match original {
        Some(orig) => unsafe { CallWindowProcW(Some(orig), hwnd, msg, wparam, lparam) },
        None => unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
        },
    }
}

/// 判断光标屏幕坐标是否落在某个可交互矩形内(物理像素口径)。
/// 未初始化或矩形为空时返回 true(整窗可交互,避免误穿透)。
#[cfg(target_os = "windows")]
fn point_in_hit_rects(hwnd: isize, lparam: isize) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;

    // 解包 WM_NCHITTEST 的 lparam:低位=屏幕 x,高位=屏幕 y
    let raw = lparam as i32;
    let sx = (raw & 0xFFFF) as i16 as i32;
    let sy = ((raw >> 16) & 0xFFFF) as i16 as i32;

    let rects = match HIT_RECTS.lock() {
        Ok(g) => g.clone(),
        Err(_) => Vec::new(),
    };
    let initialized = matches!(RECTS_INITIALIZED.lock(), Ok(g) if *g);
    if !initialized || rects.is_empty() {
        return true;
    }

    // 把屏幕坐标转成窗口客户区坐标(物理像素)
    let mut pt = POINT { x: sx, y: sy };
    unsafe {
        ScreenToClient(hwnd, &mut pt);
    }
    let (cx, cy) = (pt.x as f64, pt.y as f64);

    // 命中矩形是 CSS 逻辑像素,先按当前窗口 DPI 换算成物理像素再比对
    let scale = window_dpi_scale(hwnd);
    let physical = crate::geometry::rects_to_logical_physical(&rects, scale);

    crate::geometry::point_in_rects(&physical, cx, cy)
}

/// 子类化条目:记录每个被挂上钩子的窗口的原 WndProc,供转发消息用。
#[cfg(target_os = "windows")]
struct SubclassEntry {
    original: windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC,
}

#[cfg(target_os = "windows")]
static SUBCLASSED: std::sync::LazyLock<Mutex<std::collections::HashMap<isize, SubclassEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// 对指定 HWND 子类化挂 WM_NCHITTEST 回调(幂等)。
#[cfg(target_os = "windows")]
fn subclass_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWLP_WNDPROC, SetWindowLongPtrW};

    if hwnd == 0 {
        return;
    }

    // 幂等检查:已在表里说明已子类化过,跳过
    {
        let guard = match SUBCLASSED.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.contains_key(&hwnd) {
            return;
        }
    }

    let original = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
    // 防御:已经是我们的钩子(或取不到),跳过
    if original == 0 || original == hit_test_subclass as isize {
        return;
    }

    // isize → WNDPROC(Option<extern fn>) 转回函数指针,供 CallWindowProcW 使用
    let original_fn: windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC =
        unsafe {
            std::mem::transmute::<isize, windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC>(
                original,
            )
        };

    let prev = unsafe {
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, hit_test_subclass as isize)
    };
    // prev == 0 表示设置失败(如跨进程的 Chrome_RenderWidgetHostHWND),记入但不生效也无妨
    let _ = prev;

    if let Ok(mut g) = SUBCLASSED.lock() {
        g.insert(hwnd, SubclassEntry { original: original_fn });
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_child_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    _lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    let class = String::from_utf16_lossy(&buf[..len.max(0) as usize]).to_lowercase();
    // WebView2 渲染表面类名关键字(覆盖 Chrome_WidgetWin_0 / Chrome_RenderWidgetHostHWND /
    // Intermediate D3D Window 等)。只对这类窗口子类化,避免误伤无关子控件。
    let is_webview2 = class.contains("chrome_widget")
        || class.contains("render")
        || class.contains("widget")
        || class.contains("d3d")
        || class.contains("intermediate");

    if is_webview2 {
        // 直接从函数指针读静态(借用 lparam 传上下文会因借用检查麻烦,这里用全局)
        if let Ok(mut g) = ENUM_RESULTS.lock() {
            g.push(hwnd);
        }
    }
    1
}

/// 枚举回调写结果的全局(EnumChildWindows 回调里不能轻易用借用,故用静态)。
#[cfg(target_os = "windows")]
static ENUM_RESULTS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// 安装穿透:子类化父窗口 + 其下所有 WebView2 渲染子窗口。
/// 需在 WebView2 初始化完成、子 HWND 已创建后调用,否则枚举不到子窗口。
#[cfg(target_os = "windows")]
pub fn install_click_through(parent_hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    if parent_hwnd == 0 {
        return;
    }

    // 1. 子类化父窗口本身
    subclass_window(parent_hwnd);

    // 2. 枚举并子类化 WebView2 渲染子窗口
    {
        if let Ok(mut g) = ENUM_RESULTS.lock() {
            g.clear();
        }
    }
    unsafe {
        let _ = EnumChildWindows(parent_hwnd, Some(enum_child_proc), 0);
    }
    let children: Vec<isize> = {
        match ENUM_RESULTS.lock() {
            Ok(g) => g.clone(),
            Err(_) => Vec::new(),
        }
    };
    for child in children {
        subclass_window(child);
    }
}

/// 对外入口:main 窗口创建后调用,安装平台穿透。
/// Windows:SetWindowRgn 区域裁切;macOS:由 macos_pet 模块处理(见 setup_notify_interactive 分支);
/// Linux:暂未实现不规则窗口穿透(需要 X11 XShape/XFixes 或 Wayland layer-shell),
/// 此时仅打印一次友好提示,宠物窗口退化为普通置顶透明窗口(无局部穿透)。
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn setup_notify_interactive(_app: &tauri::AppHandle) {
    eprintln!(
        "[windows_pet] Linux 暂不支持不规则窗口穿透:宠物窗口将以普通置顶透明窗口显示,\
         点击宠物区域外也会命中窗口(无穿透)。如需完整支持,请参考 X11 XShape/XFixes 方案。"
    );
}