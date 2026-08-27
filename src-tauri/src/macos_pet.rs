// macOS 专用：桌面宠物（main 窗口）鼠标交互 + 透明区点击穿透。
// 仅 target_os = "macos" 编译；Windows 完全不加载本模块，Windows 行为不变。
//
// 设计要点（hover 与穿透用同一套 NSTimer 轮询，互不干扰）：
//   A) 透明区点击穿透：整窗默认可交互，但 NSTimer 每 50ms 用 NSEvent.mouseLocation
//      （不受 app/window 激活态影响，永远拿到真实鼠标坐标）换算到窗口内容区 CSS
//      坐标，判断鼠标是否在「可交互矩形（宠物+气泡）」内：
//        - 命中 → setIgnoresMouseEvents:false + makeKeyWindow（点击/拖拽进 webview，
//          且拖动一次即生效，无需先点激活窗口）
//        - 未命中 → setIgnoresMouseEvents:true（透明区点击穿透到下层桌面/窗口）
//   B) hover（鼠标移上去触发动作）：同样由 NSTimer 用 mouseLocation 判定 inside，
//      状态变化时才 emit "pet-mouse-hover" 事件给前端。hover 完全不依赖浏览器原生
//      mouseenter（不受 AppKit 激活态影响），因此「穿透切换」不会让它失效。
//
// 之所以穿透用「整窗 ignoresMouseEvents 切换」而非「hitTest 返回 nil」：
//   后者需要给 Tauri 的 NSWindow 实例 object_setClass 换子类，风险较高；
//   而 ignoresMouseEvents 切换是成熟的 AppKit 做法，且与 NSTimer 驱动的 hover
//   配合后，透明区穿透与 hover 稳定可同时成立。

use std::sync::Mutex;

// 可交互矩形（CSS 坐标，相对 main 窗口内容区左上角）：(x, y, w, h)
type Rect = (f64, f64, f64, f64);
static INTERACTIVE_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
static NS_WINDOW_PTR: Mutex<usize> = Mutex::new(0);
static APP_HANDLE: Mutex<Option<tauri::AppHandle>> = Mutex::new(None);
static LAST_HOVER: Mutex<Option<bool>> = Mutex::new(None);
static INSTALLED: Mutex<bool> = Mutex::new(false);
// 启动初期 Tauri 会重置窗口 styleMask / activationPolicy，导致我们 early 设的
// nonactivating panel / Accessory 被覆盖（现象：首次打开点宠物仍激活 app 变灰，
// 直到打开设置菜单那个更晚的时机才真正生效）。用「自愈计数」在前若干帧持续确保，
// 覆盖 Tauri 初始化窗口期；设置完成后停止（避免每帧反复 set 的额外开销）。
static ENSURE_FRAMES: Mutex<i32> = Mutex::new(200); // 200 帧 ≈ 10s，足够覆盖启动初始化

/// 前端调用：更新可交互区域列表（宠物 + 气泡矩形，CSS 坐标）。
/// macOS 下这些矩形用于 NSTimer 轮询判断「鼠标是否在宠物/气泡上」，
/// 驱动穿透切换与 "pet-mouse-hover" 事件。
#[tauri::command]
pub fn set_notify_interactive_rects(rects: Vec<(f64, f64, f64, f64)>) {
    let mut g = INTERACTIVE_RECTS.lock().unwrap_or_else(|e| e.into_inner());
    *g = rects;
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::APP_HANDLE;
    use super::ENSURE_FRAMES;
    use super::INSTALLED;
    use super::INTERACTIVE_RECTS;
    use super::LAST_HOVER;
    use super::NS_WINDOW_PTR;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, class, sel, ClassType};
    use objc2_foundation::{NSObject, NSPoint, NSRect};
    use tauri::{Emitter, Manager};

    // NSTimer target：每 50ms 计算 hover + 切换透明区穿透。
    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "PetMouseTimerTarget"]
        struct PetMouseTimerTarget;

        impl PetMouseTimerTarget {
            #[unsafe(method(tick:))]
            fn tick(&self, _timer: *mut AnyObject) {
                unsafe {
                    let window_ptr = match NS_WINDOW_PTR.lock() {
                        Ok(g) => *g,
                        Err(_) => return,
                    };
                    if window_ptr == 0 {
                        return;
                    }
                    let window = window_ptr as *mut AnyObject;

                    // ── 自愈：启动初期持续确保 nonactivating panel + Accessory ──
                    // 现象：首次打开点宠物仍会激活 app（变灰），直到打开设置菜单
                    // （更晚的时机）才生效 —— 说明 Tauri 在窗口创建后重置了 styleMask
                    // 与 activationPolicy，早期设置被覆盖。这里在前 ENSURE_FRAMES 帧
                    // 每帧纠正，覆盖其初始化窗口期；之后停止。
                    {
                        let mut frames = ENSURE_FRAMES.lock().unwrap_or_else(|e| e.into_inner());
                        if *frames > 0 {
                            *frames -= 1;
                            // 1) 窗口 styleMask 加 NSNonactivatingPanelMask（1<<7）：
                            //    点击窗口不激活 owning app，但 nonactivating panel 仍接收
                            //    鼠标事件（拖拽/hover 照常）。这是 ChatGPT 桌面端浮动窗标准做法。
                            let mask: u64 = msg_send![window, styleMask];
                            if mask & (1u64 << 7) == 0 {
                                let _: () = msg_send![window, setStyleMask: mask | (1u64 << 7)];
                            }
                            // 2) app 设为 Accessory（不显示 Dock、点击窗口不激活 app）
                            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                            let _: () = msg_send![app, setActivationPolicy: 1i64]; // Accessory=1
                        }
                    }

                    // 全局鼠标（屏幕坐标，左下原点）
                    let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
                    // 窗口 frame（屏幕坐标，左下原点）
                    let wframe: NSRect = msg_send![window, frame];
                    // 换算到窗口内容区 CSS 坐标（左上原点，与前端 getBoundingClientRect 同一空间）
                    let css_x = mouse.x - wframe.origin.x;
                    let css_y = wframe.size.height - (mouse.y - wframe.origin.y);

                    let rects = INTERACTIVE_RECTS.lock().unwrap_or_else(|e| e.into_inner());
                    let inside = rects
                        .iter()
                        .any(|&(rx, ry, rw, rh)| css_x >= rx && css_x < rx + rw && css_y >= ry && css_y < ry + rh);

                    // 命中可交互区 → 窗口可交互（点击/拖拽进 webview）；
                    // 未命中（透明区）→ 穿透到下层桌面/窗口。
                    // 注意：ignoresMouseEvents 切换不影响下面的 hover 判定
                    // （hover 由 mouseLocation 直接算，不依赖 webview 的原生 mousemove）。
                    // 切忌调用 makeKeyWindow：它会把 Accessory app 带到前台、抢走当前
                    // app 的焦点（点宠物时浏览器等会失焦）。Accessory + ignoresMouseEvents:false
                    // + tauri.conf 的 acceptFirstMouse 已足够让点击/拖拽直接生效。
                    let _: () = msg_send![window, setIgnoresMouseEvents: !inside];

                    // hover：仅在状态变化时推送前端
                    let mut last = LAST_HOVER.lock().unwrap_or_else(|e| e.into_inner());
                    if last.map_or(true, |v| v != inside) {
                        *last = Some(inside);
                        if let Some(app) = APP_HANDLE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                            let _ = app.emit("pet-mouse-hover", inside);
                        }
                    }
                }
            }
        }
    );

    pub fn install(app: &tauri::AppHandle) -> bool {
        {
            let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
            if *installed {
                return true;
            }
            *installed = true;
        }

        // 保存 AppHandle 供 timer 推送 hover 事件
        {
            let mut g = APP_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
            *g = Some(app.clone());
        }

        let Some(w) = app.get_webview_window("main") else {
            return false;
        };
        let Ok(ns_window) = w.ns_window() else {
            return false;
        };
        let ns_window_ptr = ns_window as *mut AnyObject as usize;
        {
            let mut g = NS_WINDOW_PTR.lock().unwrap_or_else(|e| e.into_inner());
            *g = ns_window_ptr;
        }

        unsafe {
            // 关键：把窗口设为 nonactivating panel（NSNonactivatingPanelMask = 1<<7）。
            // 这是 ChatGPT 桌面端等浮动助手的标准做法——窗口点击【不会激活 owning app】，
            // 因此当前前台 app（如浏览器）保持焦点不变灰；同时 nonactivating panel 仍能
            // 正常接收鼠标事件，拖拽/hover 照常生效。仅加 bit 不改窗口类，风险低。
            let mask: u64 = msg_send![ns_window_ptr as *mut AnyObject, styleMask];
            let new_mask = mask | (1u64 << 7); // NSWindowStyleMaskNonactivatingPanel
            let _: () = msg_send![ns_window_ptr as *mut AnyObject, setStyleMask: new_mask];

            // 初始状态：保持可交互（false），避免启动瞬间被误判穿透导致无法点击。
            let _: () = msg_send![ns_window_ptr as *mut AnyObject, setIgnoresMouseEvents: false];
            let _: () = msg_send![ns_window_ptr as *mut AnyObject, setAcceptsMouseMovedEvents: true];
            // 注意：不调用 makeKeyWindow，避免 Accessory app 被带到前台抢焦点
            // （点宠物时当前 app 应保持在前台）。

            // 启动 NSTimer 每 50ms：计算 hover + 切换透明区穿透
            let cls = PetMouseTimerTarget::class();
            let target_obj: *mut AnyObject = msg_send![cls, alloc];
            let target_obj: *mut AnyObject = msg_send![target_obj, init];
            let _timer: *mut AnyObject = msg_send![
                class!(NSTimer),
                scheduledTimerWithTimeInterval: 0.05f64,
                target: &*target_obj,
                selector: sel!(tick:),
                userInfo: std::ptr::null::<AnyObject>(),
                repeats: true
            ];
        }
        true
    }
}

/// 对外入口：main 窗口创建后调用。
#[cfg(target_os = "macos")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    let _ = macos_impl::install(app);
}
