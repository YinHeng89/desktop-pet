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
//
// === 本版相对上一版修复的问题 ===
// 1.(致命)WebView2 在 Tauri 窗口内部往往不止一层子 HWND(常见是
//    Chrome_WidgetWin_0 宿主 + Chrome_WidgetWin_1 真正接收输入的渲染窗口,
//    某些环境下还有更深层级)。旧代码 pick_webview_child 只挑第一个匹配窗口子类化,
//    但真正收到 WM_NCHITTEST 的往往是另一层未被子类化的窗口,导致命中测试在那一层
//    直接以默认的 HTCLIENT 结束,消息根本传导不到我们写的逻辑——表现为「整个透明区域
//    都不能穿透」。现在改为:枚举到的所有 class 匹配的子孙窗口全部子类化。
// 2. 旧代码「找到一个就 KillTimer」,但 WebView2 的多层子窗口是分批异步创建的,
//    过早停表会导致后来才出现的那一层永远没机会被子类化。现在改为:达到「连续两轮
//    扫描都没有新窗口」才降频,且不完全停表,长期低频保底重扫,应对 WebView2 导航/
//    重建产生新的子窗口 HWND。
// 3.(安全)旧代码从未在窗口销毁时移除子类、清理定时器和静态状态。子类回调持有
//    ref_data(主窗口 HWND)及读取全局静态,如果主窗口 HWND 被系统回收并被其他窗口
//    复用,残留的 subclass/timer 可能读到脏状态甚至引发未定义行为。现在在
//    install_hit_test_subclass 里注册窗口销毁清理,并提供 teardown_hit_test_subclass
//    供 Tauri 的 window destroyed 事件调用。
// 4. 重新调用 install_hit_test_subclass(例如窗口被重建)时,旧代码不会清理上一轮的
//    WEBVIEW_CHILD_HWND / 定时器,可能重复安装或对失效句柄操作。现在统一在安装前做
//    一次 teardown。

use std::sync::Mutex;

// Manager 特性提供 app.get_webview_window(...),在所有平台都需要
use tauri::Manager;

// Windows 专属 API(仅 Windows 编译,避免在其他平台拉入 windows-sys 符号)。
// 命中测试由 WM_NCHITTEST 子类完成,不再依赖 SetWindowRgn。
// SetWindowSubclass / DefSubclassProc / RemoveWindowSubclass 来自 comctl32,
// 在 windows-sys 0.52 中位于 Win32::UI::Shell 模块(非 Controls),
// 需启用 Win32_UI_Shell 特性。
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetClientRect, HTCLIENT, HTTRANSPARENT, IsWindow, KillTimer,
    SetTimer, WM_NCDESTROY, WM_NCHITTEST,
};
// ScreenToClient 在 windows-sys 0.52 中归在 Win32::Graphics::Gdi 模块(而非 WindowsAndMessaging)。
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};

// 可交互矩形(CSS 逻辑像素,相对窗口内容区/视口左上角):(x, y, w, h)
// 复用跨平台共享类型,保证与 macOS 端语义一致。
use crate::geometry::Rect;
static HIT_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 是否已初始化(前端上报过矩形)。未初始化时保持整窗可交互。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

#[cfg(target_os = "windows")]
const LOGPIXELSX: i32 = 88; // GDI 常量:每逻辑英寸像素数(X 方向),仅兜底路径用
#[cfg(target_os = "windows")]
const HITTEST_SUBCLASS_ID: usize = 1; // 主窗口 WM_NCHITTEST 子类 id
#[cfg(target_os = "windows")]
const CHILD_SUBCLASS_ID: usize = 2; // WebView2 子窗口 WM_NCHITTEST 子类 id
#[cfg(target_os = "windows")]
const RETRY_TIMER_ID: usize = 0x5001; // 重试子类化 WebView2 子窗口的定时器 id
#[cfg(target_os = "windows")]
const RETRY_INTERVAL_FAST_MS: u32 = 300; // 尚未找到/仍在增长阶段的扫描间隔
#[cfg(target_os = "windows")]
const RETRY_INTERVAL_SLOW_MS: u32 = 2000; // 已稳定后的低频保底扫描间隔(应对导航/重建)
#[cfg(target_os = "windows")]
const STABLE_ROUNDS_BEFORE_SLOWDOWN: u32 = 2; // 连续几轮没有新增子窗口后转入低频

// 已子类化的 WebView2 相关子窗口集合(可能不止一个,见文件头说明)。
#[cfg(target_os = "windows")]
static WEBVIEW_CHILDREN: Mutex<Vec<isize>> = Mutex::new(Vec::new());
// 供重试定时器回调读取主窗口 HWND(TimerProc 回调里没有 ref_data 可用)。
#[cfg(target_os = "windows")]
static MAIN_HWND_FOR_TIMER: Mutex<isize> = Mutex::new(0);
// 连续多少轮扫描没有发现新的子窗口(用于判断是否可以降频)。
#[cfg(target_os = "windows")]
static STABLE_ROUNDS: Mutex<u32> = Mutex::new(0);

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
/// 存为静态,WM_NCHITTEST 子类在每次命中测试时实时读取,无需再显式"应用"到窗口。
/// 空数组表示尚未渲染出有效元素,此时把 RECTS_INITIALIZED 置回 false,
/// 避免「已初始化但矩形为空」导致整窗永久穿透。
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
/// 负责,命中测试由 WM_NCHITTEST 子类负责),因此隐藏只需直接 hide(),无需先清 region,
/// 也不会有旧代码担心的"残留小 region 导致缩略图"问题。
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
            // 安装 WM_NCHITTEST 子类:窗口保持整块透明矩形(可见形状由 WebView2 的
            // 逐像素 alpha 决定,不再用 SetWindowRgn 裁切),命中测试改由子类在点击时
            // 实时读取 HIT_RECTS 完成——彻底消除「region 提前生效 vs Chromium 帧提交」
            // 的竞态,多实例并发时各窗口也独立稳定。
            install_hit_test_subclass(hwnd_isize);

            // 窗口关闭/销毁时清理子类与定时器,避免残留回调在 HWND 被系统回收复用后
            // 读到脏的全局状态。Tauri 的 WebviewWindow 支持监听窗口事件。
            let cleanup_hwnd = hwnd_isize;
            w.on_window_event(move |event| {
                if let tauri::WindowEvent::Destroyed = event {
                    teardown_hit_test_subclass(cleanup_hwnd);
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
/// 设置窗口是普通无边框卡片窗口,需要「圆角 + 悬浮投影」的精致外观。
/// 若用 `SetWindowRgn` 自绘圆角,会裁掉整个窗口 shape 之外的像素 —— 包括系统投影,
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
/// WM_NCHITTEST 子类负责(见 install_hit_test_subclass,不再用 SetWindowRgn 裁切),
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

/// 在 main 窗口(及其所有 WebView2 相关子孙窗口)上安装 WM_NCHITTEST 子类,实现
/// 「宠物/气泡可交互,区域外点击穿透到桌面」,且**不**使用 SetWindowRgn 裁切可见形状。
///
/// 为什么必须子类化**所有**匹配的子孙窗口(这是本版相对旧版的关键修复):
/// WebView2 在 Tauri 窗口内部通常不止一层 HWND,常见结构类似:
///   main_hwnd
///     └─ Chrome_WidgetWin_0   (WebView2 宿主/控制器窗口)
///          └─ Chrome_WidgetWin_1  (Chromium 真正接收鼠标输入的渲染窗口)
///               └─(某些环境下还有更深层)
/// Windows 处理 WM_NCHITTEST 时,是从当前屏幕坐标点、z-order 最靠上的那个窗口开始测试,
/// 只有它返回 HTTRANSPARENT,系统才会继续往下一层测试。如果只子类化了外层的 `_0`,
/// 而真正接收输入的是内层的 `_1`,消息在 `_1` 这一层就已经被 Chromium 的默认处理
/// (返回 HTCLIENT)截断,永远不会传导到我们子类化的窗口——表现为整个透明区域都
/// 无法穿透。因此这里把 EnumChildWindows 递归枚举到的所有 class 匹配窗口全部子类化,
/// 不管点击实际落在哪一层,都能被拦截并正确返回 HTTRANSPARENT/HTCLIENT,逐层冒泡到
/// main_hwnd。
///
/// 关键时机问题:WebView2 的子 HWND 是**异步、分批**初始化的——Tauri 主窗口创建后,
/// WebView2 environment/controller 才在另一个线程逐步把子窗口挂上来,不同层级出现的
/// 时间点可能不同。如果只扫描一次或"找到第一个就停表",很可能漏掉后来才出现的那一层。
/// 因此用定时器周期性重试:未稳定前高频扫描,连续多轮没有新增子窗口后转入低频保底扫描
/// (而不是完全停表),以应对 WebView2 导航/重建产生新的子窗口 HWND。
#[cfg(target_os = "windows")]
pub fn install_hit_test_subclass(main_hwnd: isize) {
    if main_hwnd == 0 {
        return;
    }
    unsafe {
        // 若之前对同一 HWND(或残留的旧状态)安装过,先清理,避免重复子类化 /
        // 定时器重复注册 / 状态串扰(例如窗口被重建后重新调用本函数的场景)。
        teardown_hit_test_subclass(main_hwnd);

        if let Ok(mut m) = MAIN_HWND_FOR_TIMER.lock() {
            *m = main_hwnd;
        }
        if let Ok(mut r) = STABLE_ROUNDS.lock() {
            *r = 0;
        }

        // 1) 主窗口自身立即子类化(同步可用)
        let _ = SetWindowSubclass(
            main_hwnd,
            Some(hit_test_subclass),
            HITTEST_SUBCLASS_ID,
            main_hwnd as usize,
        );
        // 2) 立即尝试子类化当前已存在的 WebView2 子孙窗口(可能只有部分已就绪)
        rescan_and_subclass_children(main_hwnd);
        // 3) 启动重试定时器:高频扫描,直到连续多轮没有新增子窗口再降频,
        //    但不会完全停表,以应对后续导航/重建产生的新子窗口。
        let _ = SetTimer(
            main_hwnd,
            RETRY_TIMER_ID,
            RETRY_INTERVAL_FAST_MS,
            Some(retry_timer_proc),
        );
    }
}

/// 卸载 main 窗口上安装的 WM_NCHITTEST 子类、定时器与相关静态状态。
/// 应在窗口销毁(WM_NCDESTROY / Tauri WindowEvent::Destroyed)时调用,
/// 防止残留回调在 HWND 被系统回收复用后读取到脏状态。
#[cfg(target_os = "windows")]
pub fn teardown_hit_test_subclass(main_hwnd: isize) {
    if main_hwnd == 0 {
        return;
    }
    unsafe {
        let _ = KillTimer(main_hwnd, RETRY_TIMER_ID);
        if IsWindow(main_hwnd) != 0 {
            let _ = RemoveWindowSubclass(main_hwnd, Some(hit_test_subclass), HITTEST_SUBCLASS_ID);
        }
        if let Ok(mut children) = WEBVIEW_CHILDREN.lock() {
            for &h in children.iter() {
                if IsWindow(h) != 0 {
                    let _ = RemoveWindowSubclass(h, Some(hit_test_subclass), CHILD_SUBCLASS_ID);
                }
            }
            children.clear();
        }
    }
    if let Ok(mut m) = MAIN_HWND_FOR_TIMER.lock() {
        if *m == main_hwnd {
            *m = 0;
        }
    }
    if let Ok(mut r) = STABLE_ROUNDS.lock() {
        *r = 0;
    }
}

/// 扫描 main_hwnd 的所有子孙窗口,把新出现的、尚未子类化的 WebView2 相关窗口
/// 全部子类化。返回本轮新增子类化的数量,供调用方判断是否已经"稳定"。
#[cfg(target_os = "windows")]
unsafe fn rescan_and_subclass_children(main_hwnd: isize) -> usize {
    if main_hwnd == 0 {
        return 0;
    }

    let mut collector = ChildCollector {
        matches: Vec::new(),
        all: Vec::new(),
    };
    EnumChildWindows(
        main_hwnd,
        Some(collect_child),
        &mut collector as *mut _ as LPARAM,
    );

    // 优先只子类化 class 名匹配 WebView2/Chrome_WidgetWin 的窗口;
    // 如果一个都没匹配到(极少见,比如未来 WebView2 改了类名),
    // 兜底子类化全部子窗口,保证至少不会出现"完全无法穿透"的情况。
    let candidates: Vec<isize> = if !collector.matches.is_empty() {
        collector.matches
    } else {
        collector.all
    };

    let mut children = WEBVIEW_CHILDREN
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // 清理掉已失效的旧句柄(WebView2 重建后旧 HWND 会失效)
    children.retain(|&h| IsWindow(h) != 0);

    let mut newly_added = 0usize;
    for h in candidates {
        if IsWindow(h) == 0 {
            continue;
        }
        if children.contains(&h) {
            continue; // 已经子类化过,跳过
        }
        let ok = SetWindowSubclass(h, Some(hit_test_subclass), CHILD_SUBCLASS_ID, main_hwnd as usize);
        if ok != 0 {
            children.push(h);
            newly_added += 1;
        }
    }
    newly_added
}

/// SetTimer 的 TimerProc 回调:周期性重新扫描并子类化新出现的 WebView2 子孙窗口。
/// 不直接用 WM_TIMER 是因为它依赖我们的 subclass 收到该消息,而本定时器跑在独立回调里,
/// 读取 MAIN_HWND_FOR_TIMER 即可拿到主窗口 HWND。
///
/// 连续 STABLE_ROUNDS_BEFORE_SLOWDOWN 轮没有发现新窗口后,把扫描间隔从高频降到低频,
/// 而不是完全停表——低频保底扫描能应对 WebView2 导航或内部重建产生的新子窗口,
/// 成本很低(几秒一次)但避免了"过早停表导致后来者永远漏子类化"的问题。
#[cfg(target_os = "windows")]
unsafe extern "system" fn retry_timer_proc(hwnd: HWND, _msg: u32, _id: usize, _time: u32) {
    let main = MAIN_HWND_FOR_TIMER.lock().map(|g| *g).unwrap_or(0);
    if main == 0 || IsWindow(main) == 0 {
        // 主窗口已不存在,自己把定时器杀掉,避免野回调持续触发
        let _ = KillTimer(hwnd, RETRY_TIMER_ID);
        return;
    }

    let newly_added = rescan_and_subclass_children(main);

    let mut rounds = STABLE_ROUNDS.lock().unwrap_or_else(|e| e.into_inner());
    if newly_added == 0 {
        *rounds += 1;
    } else {
        *rounds = 0;
    }

    if *rounds == STABLE_ROUNDS_BEFORE_SLOWDOWN {
        // 刚达到稳定阈值:切换到低频保底扫描
        let _ = KillTimer(main, RETRY_TIMER_ID);
        let _ = SetTimer(main, RETRY_TIMER_ID, RETRY_INTERVAL_SLOW_MS, Some(retry_timer_proc));
    }
}

/// WM_NCHITTEST 子类回调:区域外返回 HTTRANSPARENT(穿透),区域内返回 HTCLIENT(可交互)。
/// ref_data 传入主窗口 HWND(usize),用于把屏幕坐标换算成窗口内容区 CSS 逻辑坐标——
/// 无论这个子类装在 main_hwnd 本身还是某一层 WebView2 子窗口上,坐标换算基准都
/// 统一用主窗口,保证与 HIT_RECTS(相对窗口内容区)语义一致。
///
/// 同时处理 WM_NCDESTROY:窗口即将销毁时,主动移除自己的子类,避免野指针/野回调
/// (如果调用方忘了显式调用 teardown_hit_test_subclass,这里作为最后一道保险)。
#[cfg(target_os = "windows")]
unsafe extern "system" fn hit_test_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    ref_data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(hit_test_subclass), id);
        if let Ok(mut children) = WEBVIEW_CHILDREN.lock() {
            children.retain(|&h| h != hwnd);
        }
        return DefSubclassProc(hwnd, msg, wparam, lparam);
    }

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
                    let r = HIT_RECTS.lock().map(|g| g.clone()).unwrap_or_default();
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
        // 坐标转换失败兜底:整窗可交互,避免用户完全无法交互
        return HTCLIENT as LRESULT;
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// EnumChildWindows 回调:收集所有子孙窗口(EnumChildWindows 本身即递归枚举),
/// 并单独标记类名匹配 WebView2/Chromium 的窗口。
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

/// 兜底工具函数,消除极少数环境下未匹配任何 class 的情况——目前逻辑已内联到
/// rescan_and_subclass_children,保留此签名占位以兼容旧调用点(如有)。
#[cfg(target_os = "windows")]
#[allow(dead_code)]
unsafe fn get_client_area(h: HWND) -> Option<(i32, i32)> {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if GetClientRect(h, &mut r) != 0 {
        Some((r.right - r.left, r.bottom - r.top))
    } else {
        None
    }
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