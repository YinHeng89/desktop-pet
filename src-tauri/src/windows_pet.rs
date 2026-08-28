// Windows 专用:桌面宠物(main 窗口)的可交互区域鼠标穿透。
// 思路与 macOS 的 macos_pet.rs(NSTimer + ignoresMouseEvents)彻底对齐:定时器轮询
// 鼠标位置,命中 HIT_RECTS 内 → 整窗恢复可交互;命中外 → 整窗对鼠标透明,点击穿透到
// 桌面下层。不再尝试对 WebView2 的任何子窗口做命中测试拦截。
//
// === 为什么放弃了上一版 WM_NCHITTEST 子类方案(重要背景,别再往那个方向改)===
// WebView2 真正接收鼠标消息、承载网页内容的那个 Chrome_WidgetWin_* 窗口,是由
// WebView2 自己的浏览器进程(msedgewebview2.exe)创建的,只是通过 SetParent 挂到了
// 我们 Tauri 窗口下面显示。它的 HWND 虽然在窗口树里表现为我们主窗口的子孙窗口,
// 但**窗口过程(WndProc)运行在它自己进程的地址空间里**。
// SetWindowSubclass(以及底层的 SetWindowLongPtr(GWLP_WNDPROC,...))替换的是"窗口过程
// 函数指针",这个操作只在窗口的宿主进程内部有意义——对一个属于别的进程的 HWND 调用它,
// 要么直接失败,要么即使调用"成功"也不会有任何效果,因为 WM_NCHITTEST 是在
// msedgewebview2.exe 自己的消息循环里处理和分发的,我们进程里的回调地址对它来说毫无
// 意义,根本不会被执行。这就是"不管怎么改子窗口枚举范围,穿透依然不生效"的根本原因:
// 问题不在枚举/覆盖是否全面,而在于跨进程子类化这条路从设计上就走不通。
//
// 正确思路:完全不碰 WebView2 的任何窗口,只操作**我们自己拥有**的顶层窗口(main_hwnd)。
// 用定时器轮询 GetCursorPos → 换算成窗口内容区 CSS 逻辑坐标 → 判断是否落在 HIT_RECTS
// 内 → 动态增删整窗的 WS_EX_TRANSPARENT 扩展样式:
//   - 命中 HIT_RECTS 内:去掉 WS_EX_TRANSPARENT,整窗正常接收鼠标
//   - 命中 HIT_RECTS 外:加上 WS_EX_TRANSPARENT,鼠标事件直接穿透到桌面下层窗口,
//     完全不经过我们窗口(也不经过 WebView2),从根上避免了任何跨进程通信的需要。
// 这与 macOS 用 NSTimer 定时轮询 + setIgnoresMouseEvents 切换整窗穿透状态是同一思路,
// 只是 API 换成了 Windows 的 WS_EX_TRANSPARENT。
//
// 前提:窗口需要已经是分层窗口(WS_EX_LAYERED),Tauri 的透明窗口(transparent: true)
// 默认会带上这个样式,这里不需要我们额外设置,只需要在其基础上增删 WS_EX_TRANSPARENT。
//
// 本文件在所有平台都参与编译(不能用 #![cfg(windows)] 包整个文件,否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号)。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部,非 Windows 平台提供 no-op 实现。

use std::sync::Mutex;

// Manager 特性提供 app.get_webview_window(...),在所有平台都需要
use tauri::Manager;

#[cfg(target_os = "windows")]
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, IsWindow, IsWindowVisible, KillTimer, SetTimer,
    SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, WS_EX_TRANSPARENT,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, POINT};

// 可交互矩形(CSS 逻辑像素,相对窗口内容区/视口左上角):(x, y, w, h)
// 复用跨平台共享类型,保证与 macOS 端语义一致。
use crate::geometry::Rect;
static HIT_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 是否已初始化(前端上报过矩形)。未初始化时保持整窗可交互。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

#[cfg(target_os = "windows")]
const LOGPIXELSX: i32 = 88; // GDI 常量:每逻辑英寸像素数(X 方向),仅兜底路径用
#[cfg(target_os = "windows")]
const POLL_TIMER_ID: usize = 0x5002;
// 轮询间隔:16ms ≈ 60Hz,兼顾响应速度和 CPU 占用(GetCursorPos + 一次 ScreenToClient
// 开销很小;只有状态真正变化时才会调用 SetWindowLongPtrW/SetWindowPos)。
// 如果想进一步降低占用可以调到 25~33ms(40~30Hz),代价是穿透状态切换会有轻微延迟感。
#[cfg(target_os = "windows")]
const POLL_INTERVAL_MS: u32 = 16;

// 每个宠物窗口(可能同时开多个实例/多个 HWND)当前是否处于"穿透"状态,
// 避免每次轮询都无条件调用 SetWindowLongPtrW(状态没变化时完全跳过系统调用)。
// true = 当前已设置 WS_EX_TRANSPARENT(穿透中),false = 当前可交互。
#[cfg(target_os = "windows")]
static CLICK_THROUGH_STATE: Mutex<Option<HashMap<isize, bool>>> = Mutex::new(None);

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
/// 存为静态,轮询定时器每帧读取,无需再显式"应用"到窗口。
/// 空数组表示尚未渲染出有效元素,此时把 RECTS_INITIALIZED 置回 false,
/// 避免「已初始化但矩形为空」导致整窗永久穿透。
///
/// 向后兼容:旧命令名。后续前端统一走 update_interactive_rects。
#[tauri::command]
pub fn set_pet_hit_rects(rects: Vec<Rect>) {
    store_hit_rects(&rects);
}

/// 前端显式触发:在当前架构下为 no-op。
/// 命中测试由轮询定时器持续进行,不需要前端主动"应用"这一步。保留此 command 仅为
/// 前端兼容,无需再调用。
#[tauri::command]
pub fn apply_pet_hit_rects(app: tauri::AppHandle) {
    let _ = app;
}

/// 隐藏 main 宠物窗口。
/// 隐藏期间轮询定时器仍在跑,但 poll_timer_proc 里会检查 IsWindowVisible 并跳过实际
/// 处理,不会有副作用,也不需要在这里额外停表(下次 show 时状态会自然重新收敛)。
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
            let hwnd_isize = hwnd.0 as isize;
            install_click_through_polling(hwnd_isize);

            // 窗口销毁时清理定时器与状态,避免残留回调在 HWND 被系统回收复用后
            // 继续跑或读到脏状态。
            let cleanup_hwnd = hwnd_isize;
            w.on_window_event(move |event| {
                if let tauri::WindowEvent::Destroyed = event {
                    teardown_click_through_polling(cleanup_hwnd);
                }
            });
        }
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn setup_notify_interactive_windows_stub() {}

/// 给**设置窗口**设置系统级圆角 + 系统级阴影(Windows, DWM 方案)。
///
/// 设置窗口是普通无边框卡片窗口,需要「圆角 + 悬浮投影」的精致外观。若用
/// `SetWindowRgn` 自绘圆角,会裁掉整个窗口 shape 之外的像素 —— 包括系统投影,
/// 导致投影消失(之前踩过的坑)。因此设置窗口改用 DWM 方案:
///
/// 1. `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUNDLARGE`(系统最大圆角档,~8px),
///    由 DWM 在系统层裁切窗口四角,且**不裁掉投影**(投影由 DWM 合成在 shape 之外)。
/// 2. `DWMWA_SHADOW = 2`(开启系统默认阴影),让无边框窗口获得悬浮投影。
///
/// 这样圆角与 CSS `--radius-window`(已改为 8px)在视觉上一致,且投影由系统绘制、不被裁。
///
/// 注意:DWM 档位只能选系统预设(大圆角档实测约 8px,无法精确到任意像素),这正是
/// 本项目把设置窗圆角定为 8px 而非 14px 的原因。宠物主窗口的命中测试穿透现在由
/// 定时器轮询 + WS_EX_TRANSPARENT 负责(见 install_click_through_polling),
/// 与本函数互不影响。
#[cfg(target_os = "windows")]
pub fn setup_window_rounded_corners(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    if hwnd == 0 {
        return;
    }

    // DWMWINDOWATTRIBUTE 枚举值(硬编码避免不同 windows-sys 版本命名差异):
    //   DWMWA_WINDOW_CORNER_PREFERENCE = 33
    //   DWMWA_SHADOW                  = 2   (开启/关闭系统阴影)
    // DWM_WINDOW_CORNER_PREFERENCE:
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

/// 安装轮询定时器:周期性读取鼠标位置,判断是否落在 HIT_RECTS 内,并据此增删
/// main_hwnd 的 WS_EX_TRANSPARENT 扩展样式。全程只操作我们自己拥有的顶层窗口,
/// 不涉及任何跨进程操作,天然绕开了 WebView2 子窗口属于别的进程这个限制。
#[cfg(target_os = "windows")]
pub fn install_click_through_polling(main_hwnd: isize) {
    if main_hwnd == 0 {
        return;
    }
    unsafe {
        // 若之前对同一 HWND(或残留的旧状态)安装过,先清理,避免定时器重复注册
        // (例如窗口被重建后重新调用本函数的场景)。
        teardown_click_through_polling(main_hwnd);

        // 初始状态:未收到任何矩形前保持整窗可交互(等效"不穿透"),
        // 与 RECTS_INITIALIZED=false 时的兜底语义一致。
        {
            let mut guard = CLICK_THROUGH_STATE.lock().unwrap_or_else(|e| e.into_inner());
            let map = guard.get_or_insert_with(HashMap::new);
            map.insert(main_hwnd, false);
        }

        let _ = SetTimer(main_hwnd, POLL_TIMER_ID, POLL_INTERVAL_MS, Some(poll_timer_proc));
    }
}

/// 卸载轮询定时器与相关状态。应在窗口销毁时调用,避免残留定时器回调在 HWND
/// 被系统回收复用后继续触发或读到脏状态。同时会把窗口样式恢复为可交互
/// (去掉 WS_EX_TRANSPARENT),避免极端情况下 hwnd 被复用时继承了穿透状态。
#[cfg(target_os = "windows")]
pub fn teardown_click_through_polling(main_hwnd: isize) {
    if main_hwnd == 0 {
        return;
    }
    unsafe {
        let _ = KillTimer(main_hwnd, POLL_TIMER_ID);
        if IsWindow(main_hwnd) != 0 {
            set_click_through(main_hwnd, false);
        }
    }
    if let Ok(mut guard) = CLICK_THROUGH_STATE.lock() {
        if let Some(map) = guard.as_mut() {
            map.remove(&main_hwnd);
        }
    }
}

/// SetTimer 的 TimerProc 回调:每次触发读取一次鼠标位置,换算为窗口内容区的
/// CSS 逻辑坐标,判断是否命中 HIT_RECTS,并据此切换 WS_EX_TRANSPARENT。
///
/// hwnd 参数由 Windows 保证等于我们调用 SetTimer 时传入的窗口句柄
/// (前提是该窗口所在线程的消息循环仍在正常泵消息,Tauri/webview2 的主线程满足这点)。
#[cfg(target_os = "windows")]
unsafe extern "system" fn poll_timer_proc(hwnd: HWND, _msg: u32, _id: usize, _time: u32) {
    if hwnd == 0 || IsWindow(hwnd) == 0 {
        // 窗口已失效,自己把定时器杀掉,避免野回调持续触发
        let _ = KillTimer(hwnd, POLL_TIMER_ID);
        if let Ok(mut guard) = CLICK_THROUGH_STATE.lock() {
            if let Some(map) = guard.as_mut() {
                map.remove(&hwnd);
            }
        }
        return;
    }

    // 窗口不可见时(比如隐藏状态)跳过处理,避免不必要的系统调用;
    // 也避免在隐藏状态下误切换样式导致下次显示时状态不一致。
    if IsWindowVisible(hwnd) == 0 {
        return;
    }

    let mut screen_pt = POINT { x: 0, y: 0 };
    if GetCursorPos(&mut screen_pt) == 0 {
        return;
    }
    let mut client_pt = screen_pt;
    if ScreenToClient(hwnd, &mut client_pt) == 0 {
        return;
    }

    let scale = window_dpi_scale(hwnd);
    if scale <= 0.0 {
        return;
    }
    let css_x = client_pt.x as f64 / scale;
    let css_y = client_pt.y as f64 / scale;

    let (rects, initialized) = {
        let r = HIT_RECTS.lock().map(|g| g.clone()).unwrap_or_default();
        let i = RECTS_INITIALIZED.lock().map(|g| *g).unwrap_or(false);
        (r, i)
    };

    // 未初始化或矩形为空:整窗可交互(不穿透),避免用户完全无法交互。
    // 否则:命中矩形内 → 可交互;命中矩形外 → 穿透。
    let should_click_through =
        initialized && !rects.is_empty() && !crate::geometry::point_in_rects(&rects, css_x, css_y);

    set_click_through(hwnd, should_click_through);
}

/// 按需增删 WS_EX_TRANSPARENT 扩展样式。内部维护每个 hwnd 当前状态,状态未变化时
/// 直接跳过,避免每 16ms 都无条件调用 SetWindowLongPtrW/SetWindowPos。
#[cfg(target_os = "windows")]
unsafe fn set_click_through(hwnd: HWND, transparent: bool) {
    {
        let mut guard = CLICK_THROUGH_STATE.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.get_or_insert_with(HashMap::new);
        let entry = map.entry(hwnd).or_insert(false);
        if *entry == transparent {
            return; // 状态没变化,跳过系统调用
        }
        *entry = transparent;
    }

    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let new_style = if transparent {
        ex_style | (WS_EX_TRANSPARENT as isize)
    } else {
        ex_style & !(WS_EX_TRANSPARENT as isize)
    };
    if new_style != ex_style {
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
        // SWP_FRAMECHANGED 促使系统立即应用扩展样式变化,而不必等待下一次
        // 与尺寸/位置相关的窗口消息;SWP_NOMOVE/NOSIZE/NOZORDER/NOACTIVATE
        // 保证这次调用只影响样式,不移动、不改变大小、不改变 z-order、不抢焦点。
        SetWindowPos(
            hwnd,
            0,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// 对外入口:main 窗口创建后调用,安装平台穿透。
/// Windows:定时器轮询鼠标位置 + WS_EX_TRANSPARENT 切换(见 install_click_through_polling);
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