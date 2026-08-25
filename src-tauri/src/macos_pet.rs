// macOS 专用：桌面宠物（main 窗口）的可交互区域鼠标穿透 + 拖动支持。
// 仅 target_os = "macos" 编译；Windows 完全不加载本模块，Windows 行为不变。
//
// 原理（动态切换 ignoresMouseEvents + NSTimer 主线程轮询）：
//   1. 前端 ToastHost 通过 invoke("set_notify_interactive_rects", rects) 上报
//      「宠物 + 气泡」的矩形（CSS 坐标，相对 notify 窗口内容区左上角，单位逻辑像素）。
//   2. 用 NSTimer 在主线程 RunLoop 上每 50ms 执行一次检测：
//      读 NSEvent.mouseLocation（全局鼠标，屏幕坐标，左下原点）与 notify 窗口 frame，
//      算出鼠标在窗口内的位置（左上原点，与前端 CSS 对齐）。
//   3. 落在可交互矩形内 → setIgnoresMouseEvents:false（可拖动/hover）；
//      否则 → setIgnoresMouseEvents:true（点击穿透到下层桌面/窗口）。
//
// 所有 AppKit 调用（NSEvent.mouseLocation / NSWindow.frame / setIgnoresMouseEvents）
// 均发生在主线程（NSTimer 回调），避免「Must only be used from the main thread」崩溃。

use std::sync::Mutex;

// 可交互矩形（CSS 坐标，相对 notify 窗口内容区左上角）：(x, y, w, h)
type Rect = (f64, f64, f64, f64);
static INTERACTIVE_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 前端是否已经上报过矩形。启动初期前端尚未上报时保持窗口可交互，
// 避免 NSTimer 在矩形为空时把窗口误判为「穿透」。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

/// 前端调用：更新可交互区域列表。传空数组清空（整窗穿透）。
#[tauri::command]
pub fn set_notify_interactive_rects(rects: Vec<(f64, f64, f64, f64)>) {
    if let Ok(mut g) = INTERACTIVE_RECTS.lock() {
        *g = rects;
    }
    if let Ok(mut init) = RECTS_INITIALIZED.lock() {
        *init = true;
    }
}

/// 是否已初始化（前端上报过矩形）。未初始化时保持可交互。
#[allow(dead_code)]
fn rects_initialized() -> bool {
    match RECTS_INITIALIZED.lock() {
        Ok(g) => *g,
        // 锁中毒：保守认为已初始化，按矩形正常判断
        Err(_) => true,
    }
}

/// 判断某点（CSS 坐标，左上原点）是否落在可交互区域内。
#[allow(dead_code)]
fn hit_interactive(x: f64, y: f64) -> bool {
    let g = INTERACTIVE_RECTS.lock().ok();
    match g {
        None => false,
        Some(rects) => rects
            .iter()
            .any(|&(rx, ry, rw, rh)| x >= rx && x < rx + rw && y >= ry && y < ry + rh),
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::hit_interactive;
    use super::rects_initialized;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, class, sel, ClassType};
    use objc2_foundation::{NSPoint, NSRect, NSObject};
    use std::sync::Mutex;
    use tauri::Manager;

    // 存 notify 窗口的 NSWindow 指针（usize 跨线程传递，但所有 objc 调用都在主线程）。
    static NS_WINDOW_PTR: Mutex<usize> = Mutex::new(0);
    static INSTALLED: Mutex<bool> = Mutex::new(false);

    // 定义一个 NSObject 子类作为 NSTimer 的 target，带一个 tick 方法。
    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "PetHitTestTimerTarget"]
        struct PetTimerTarget;

        impl PetTimerTarget {
            // NSTimer 回调：每 50ms 在主线程执行一次穿透检测
            #[unsafe(method(tick:))]
            fn tick(&self, _timer: *mut AnyObject) {
                unsafe {
                    let window_ptr = match NS_WINDOW_PTR.lock() {
                        Ok(g) => *g,
                        Err(_) => 0,
                    };
                    if window_ptr == 0 {
                        return;
                    }
                    let window = window_ptr as *mut AnyObject;

                    // 全局鼠标位置（屏幕坐标，左下原点）
                    let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
                    // notify 窗口 frame（屏幕坐标，左下原点）
                    let frame: NSRect = msg_send![window, frame];

                    // 鼠标在窗口内的位置，转「左上原点」CSS 坐标（与前端对齐）：
                    let wx = mouse.x - frame.origin.x;
                    let wy = frame.size.height - (mouse.y - frame.origin.y);

                    // 前端尚未上报矩形（启动初期）→ 保持可交互，避免误穿透；
                    // 已初始化 → 按命中判断（命中可交互，未命中穿透）。
                    let should_ignore = if rects_initialized() {
                        !hit_interactive(wx, wy)
                    } else {
                        false
                    };
                    let _: () = msg_send![window, setIgnoresMouseEvents: should_ignore];
                }
            }
        }
    );

    pub fn install(app: &tauri::AppHandle) -> bool {
        {
            let mut installed = INSTALLED.lock().unwrap();
            if *installed {
                return true;
            }
            *installed = true;
        }

        let Some(w) = app.get_webview_window("main") else {
            return false;
        };
        let Ok(ns_window) = w.ns_window() else {
            return false;
        };
        let ns_window_ptr = ns_window as *mut AnyObject as usize;
        {
            let mut g = NS_WINDOW_PTR.lock().unwrap();
            *g = ns_window_ptr;
        }

        unsafe {
            // 初始状态：可交互（false = 不忽略鼠标）
            let _: () = msg_send![ns_window_ptr as *mut AnyObject, setIgnoresMouseEvents: false];

            // 创建 target 实例（NSObject 子类，作为 NSTimer 的 target）。
            // 注意：define_class! 生成的类需先调用 class() 触发注册到 runtime，
            // 否则 class!(PetTimerTarget) 会因「class not found」panic。
            let cls = PetTimerTarget::class();
            let target: *mut AnyObject = msg_send![cls, alloc];
            let target: *mut AnyObject = msg_send![target, init];
            // scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:
            // 由主线程 RunLoop 持有，自动重复触发，无需手动持有 timer。
            let _timer: *mut AnyObject = msg_send![
                class!(NSTimer),
                scheduledTimerWithTimeInterval: 0.05f64,
                target: &*target,
                selector: sel!(tick:),
                userInfo: std::ptr::null::<AnyObject>(),
                repeats: true
            ];
        }
        true
    }
}

/// 对外入口：notify 窗口创建后调用。
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = macos_impl::install(app);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}
