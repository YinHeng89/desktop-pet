// macOS 专用：桌面宠物（main 窗口）的可交互区域鼠标穿透 + 拖动支持 + 非激活窗口。
// 仅 target_os = "macos" 编译；Windows 完全不加载本模块，Windows 行为不变。
//
// 原理（动态切换 ignoresMouseEvents + NSTimer 主线程轮询）：
//   1. 前端 ToastHost 通过 invoke("set_notify_interactive_rects", rects) 上报
//      「宠物 + 气泡」的矩形（CSS 坐标，相对宠物窗口(main)内容区左上角，单位逻辑像素）。
//   2. 用 NSTimer 在主线程 RunLoop 上每 16ms 执行一次检测：
//      读 NSEvent.mouseLocation（全局鼠标，屏幕坐标，左下原点）与宠物窗口(main) frame，
//      算出鼠标在窗口内的位置（左上原点，与前端 CSS 对齐）。
//   3. 落在可交互矩形内 → setIgnoresMouseEvents:false（可拖动/hover）；
//      否则 → setIgnoresMouseEvents:true（点击穿透到下层桌面/窗口）。
//
// 非激活窗口（不抢焦点）：
//   - 覆盖 canBecomeMainWindow 返回 NO（仅宠物窗口）：点击/拖拽时窗口只会变 Key、
//     不会变 Main，因此 App 不会被激活、不会抢占其它 App 焦点；settings 窗口不受影响。
//   - setAcceptsMouseMovedEvents(true)：App 非激活时也持续接收鼠标移动事件。
//   - setCollectionBehavior(337)：全 Space 一致表现，避免切换虚拟桌面时边框闪烁。
//   - setMovable(NO) + setMovableByWindowBackground(NO)：禁用 AppKit 原生拖动，
//     窗口位置 100% 由 NSTimer 的 setFrameOrigin 决定（避免双逻辑抢窗口）。
//
// hover / drag 原生桥接（WebView 在 App 非激活时 mouseenter/mousedown 不可靠）：
//   - hover：NSTimer 命中可交互矩形 → emit "pet-hover"（true/false），前端播放 waiting。
//   - drag：用全局 pressedMouseButtons 状态 + setFrameOrigin 直接移动窗口（不依赖
//     WebView 的 mousedown），emit "pet-drag-start" / "pet-drag"（方向）/ "pet-drag-end"。
//
// 所有 AppKit 调用（NSEvent.mouseLocation / NSWindow.frame / setIgnoresMouseEvents 等）
// 均发生在主线程（NSTimer 回调），避免「Must only be used from the main thread」崩溃。

use std::sync::Mutex;

// 可交互矩形（CSS 坐标，相对宠物窗口(main)内容区左上角）：(x, y, w, h)
// 复用跨平台共享类型,保证与 Windows 端语义一致。
use crate::geometry::{point_in_rects, Rect};
static INTERACTIVE_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
// 前端是否已经上报过矩形。启动初期前端尚未上报时保持窗口可交互，
// 避免 NSTimer 在矩形为空时把窗口误判为「穿透」。
static RECTS_INITIALIZED: Mutex<bool> = Mutex::new(false);

// 前端上报新交互矩形后置 true,强制下一次 tick 重算穿透(即使鼠标/窗口未动)。
// 跨平台:store_interactive_rects(mac/win 都编译)与 tick(mac-only)共用。
static RECTS_DIRTY: Mutex<bool> = Mutex::new(true);

// ── 性能优化缓存(macOS-only,tick 用) ──
// 记录上一次处理的鼠标位置与窗口 frame origin,两者都未变化时 tick 直接跳过。
// NSPoint/NSRect 来自 objc2_foundation(mac-only 依赖),故整组仅 macOS 编译。
#[cfg(target_os = "macos")]
use objc2_foundation::NSPoint;
#[cfg(target_os = "macos")]
static LAST_MOUSE: Mutex<NSPoint> = Mutex::new(NSPoint {
    x: f64::MIN,
    y: f64::MIN,
});
#[cfg(target_os = "macos")]
static LAST_FRAME_ORIGIN: Mutex<NSPoint> = Mutex::new(NSPoint {
    x: f64::MIN,
    y: f64::MIN,
});

/// 内部：存储可交互矩形（供跨平台统一命令 update_interactive_rects 调用）。
pub(crate) fn store_interactive_rects(rects: &[Rect]) {
    let non_empty = !rects.is_empty();
    if let Ok(mut g) = INTERACTIVE_RECTS.lock() {
        *g = rects.to_vec();
    }
    if let Ok(mut init) = RECTS_INITIALIZED.lock() {
        *init = non_empty;
    }
    // 标记 rects 已更新,强制下一次 tick 重算穿透(即使鼠标/窗口未动,
    // 否则新交互区要等鼠标移动才生效)。
    if let Ok(mut d) = RECTS_DIRTY.lock() {
        *d = true;
    }
}

/// 前端调用：更新可交互区域列表。传空数组清空（整窗穿透）。
/// 注意：空矩形说明前端尚未渲染出有效交互元素（如宠物尚未加载），
/// 此时把 RECTS_INITIALIZED 置回 false，让 NSTimer 保持窗口可交互，
/// 避免「已初始化但矩形为空」导致鼠标永久穿透。
#[tauri::command]
pub fn set_notify_interactive_rects(rects: Vec<Rect>) {
    store_interactive_rects(&rects);
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
/// 复用 geometry::point_in_rects,与 Windows 端命中语义保持一致（含边界）。
#[allow(dead_code)]
fn hit_interactive(x: f64, y: f64) -> bool {
    let g = INTERACTIVE_RECTS.lock().ok();
    match g {
        None => false,
        Some(rects) => point_in_rects(&rects, x, y),
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::hit_interactive;
    use super::rects_initialized;
    use super::{LAST_FRAME_ORIGIN, LAST_MOUSE, RECTS_DIRTY};
    use core::ffi::c_char;
    use objc2::ffi::{class_getInstanceMethod, class_replaceMethod, method_getTypeEncoding};
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Method, Sel};
    use objc2::{class, define_class, msg_send, sel, ClassType};
    use objc2_foundation::{NSObject, NSPoint, NSRect};
    use std::sync::{Mutex, MutexGuard};
    use tauri::{Emitter, Manager};

    // 存宠物窗口(main)的 NSWindow 指针（usize 跨线程传递，但所有 objc 调用都在主线程）。
    static NS_WINDOW_PTR: Mutex<usize> = Mutex::new(0);
    static INSTALLED: Mutex<bool> = Mutex::new(false);
    // 用于向前端 emit 事件的 AppHandle（在 install 时保存）。
    static APP_HANDLE: Mutex<Option<tauri::AppHandle>> = Mutex::new(None);

    // hover 状态缓存：None=尚未判定过（首次 tick 才上报）。
    static PREV_OVER: Mutex<Option<bool>> = Mutex::new(None);

    // 原生 drag 状态机。
    //   DRAG_ARMED：按下左键（在可交互区域）但尚未移动超过阈值，等待确认是否真拖拽；
    //   DRAG_ACTIVE：已超过阈值，进入真正拖拽，每 tick 用 setFrameOrigin 跟随鼠标；
    //   DRAG_OFFSET：按下时鼠标相对窗口左下角的偏移（拖拽时保持不变）；
    //   DRAG_PRESS：按下瞬间的全局鼠标位置（用于判定位移阈值与方向）。
    static DRAG_ACTIVE: Mutex<bool> = Mutex::new(false);
    static DRAG_ARMED: Mutex<bool> = Mutex::new(false);
    static DRAG_OFFSET: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
    static DRAG_PRESS: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
    // 已上报的拖拽方向：Some(1)=向右，Some(-1)=向左，None=尚未判定。
    static DRAG_DIR: Mutex<Option<i8>> = Mutex::new(None);

    /// 覆盖 canBecomeMainWindow：仅宠物窗口返回 NO（可成为 Key 但永不成为 Main，
    /// 因此不会激活 App、不会抢其它 App 焦点）；其余窗口（settings）返回 YES。
    unsafe extern "C-unwind" fn pet_can_become_main_window(
        this: *mut AnyObject,
        _cmd: Sel,
    ) -> Bool {
        let pet_ptr = match NS_WINDOW_PTR.lock() {
            Ok(g) => *g,
            Err(_) => 0,
        };
        if (this as usize) == pet_ptr {
            Bool::NO
        } else {
            Bool::YES
        }
    }

    /// 取锁；锁中毒时打印并返回 None，避免在 ObjC 回调里 panic。
    ///
    /// 背景：tick 是 NSTimer 的 ObjC 回调（`extern "C-unwind"`）。
    /// panic 穿越 ObjC 栈帧时（ObjC 不是 unwind-safe）大概率直接 abort 整个进程，
    /// 而不是普通的线程 panic。而 tick 每 16ms 执行一次——
    /// 一旦某个锁中毒，`unwrap()` 会在下一个 tick 立刻二次 panic，
    /// 使 App 进入「必崩」状态。因此这里选择跳过本次 tick 而非传播 panic。
    fn lock_ok<T>(m: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
        match m.lock() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("[macos_pet] 检测到锁中毒，跳过本次 tick: {e}");
                None
            }
        }
    }

    /// tick 的实际逻辑（与 catch_unwind 包装分离，便于阅读）。
    ///
    /// 返回 `Option<()>`：任一互斥锁中毒即以 None 提前返回（等价于跳过本次 tick）。
    ///
    /// # Safety
    /// 内部全部是 objc 消息发送与原始指针解引用，调用方需保证
    /// NS_WINDOW_PTR 指向的 NSWindow 仍然存活。
    unsafe fn tick_inner() -> Option<()> {
        let window_ptr = *lock_ok(&NS_WINDOW_PTR)?;
        if window_ptr == 0 {
            return Some(());
        }
        let window = window_ptr as *mut AnyObject;

        // 取出 AppHandle 的克隆，避免在 emit 期间长时间持有锁。
        let app = lock_ok(&APP_HANDLE)?.clone();

        // 全局鼠标位置（屏幕坐标，左下原点）
        let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        // 宠物窗口(main) frame（屏幕坐标，左下原点）
        let frame: NSRect = msg_send![window, frame];

        // ── 性能优化：输入未变化时短路 ──
        // 拖拽进行中必须每帧跑(跟随鼠标)，不短路。
        // 否则仅当「鼠标位置变化」或「窗口 frame 变化」或「rects 刚更新」
        // 才继续处理；三者都未变则直接返回，跳过后续所有 objc 调用与重算
        // (尤其是 setIgnoresMouseEvents: 的属性重算与 hover 锁竞争)，
        // 让宠物静止时进程可休眠，降低常驻 CPU 占用。
        let drag_active = *lock_ok(&DRAG_ACTIVE)?;
        if !drag_active {
            let rects_dirty = {
                let mut d = lock_ok(&RECTS_DIRTY)?;
                let v = *d;
                if v {
                    *d = false; // 消费一次,下次恢复按鼠标/窗口变化判断
                }
                v
            };
            if !rects_dirty {
                let last_m = *lock_ok(&LAST_MOUSE)?;
                let last_f = *lock_ok(&LAST_FRAME_ORIGIN)?;
                let mouse_same =
                    (mouse.x - last_m.x).abs() < 0.5 && (mouse.y - last_m.y).abs() < 0.5;
                let frame_same = (frame.origin.x - last_f.x).abs() < 0.5
                    && (frame.origin.y - last_f.y).abs() < 0.5;
                if mouse_same && frame_same {
                    // 无任何输入变化:跳过本次 tick,不做穿透/hover/拖拽处理
                    return Some(());
                }
            }
        }
        // 记录当前输入,供下次比对(无论是否短路都更新,保证脏 rects 消费后
        // 下一次能用真实值判断)
        *lock_ok(&LAST_MOUSE)? = mouse;
        *lock_ok(&LAST_FRAME_ORIGIN)? = frame.origin;

        // 鼠标在窗口内的位置，转「左上原点」CSS 坐标（与前端对齐）：
        let wx = mouse.x - frame.origin.x;
        let wy = frame.size.height - (mouse.y - frame.origin.y);

        // 是否落在可交互区域（宠物 / 气泡）。
        let over = rects_initialized() && hit_interactive(wx, wy);

        // ── 1) 动态穿透：保留原有逻辑 ──
        // 前端尚未上报矩形（启动初期）→ 保持可交互，避免误穿透；
        // 已初始化 → 按命中判断（命中可交互，未命中穿透）。
        let should_ignore = if rects_initialized() { !over } else { false };
        let _: () = msg_send![window, setIgnoresMouseEvents: should_ignore];

        // ── 2) hover 桥接：命中状态变化时 emit pet-hover ──
        {
            let mut prev = lock_ok(&PREV_OVER)?;
            if *prev != Some(over) {
                *prev = Some(over);
                if let Some(a) = app.as_ref() {
                    let _ = a.emit("pet-hover", over);
                }
            }
        }

        // ── 3) 原生 drag：用全局鼠标按键状态 + setFrameOrigin 移动窗口 ──
        // NSEvent::pressedMouseButtons()（bit0=左键）在 App 非激活时也能读取，
        // 不依赖 WebView 是否收到 mousedown，因此「第一次按下」即可拖拽。
        {
            let buttons: usize = msg_send![class!(NSEvent), pressedMouseButtons];
            let left_down = (buttons & 1) != 0;

            let mut active = lock_ok(&DRAG_ACTIVE)?;
            let mut armed = lock_ok(&DRAG_ARMED)?;
            let mut offset = lock_ok(&DRAG_OFFSET)?;
            let mut press = lock_ok(&DRAG_PRESS)?;
            let mut dir = lock_ok(&DRAG_DIR)?;

            if left_down {
                if *active {
                    // 已在拖拽：窗口跟随鼠标（保持按下时的偏移）
                    let o = NSPoint {
                        x: mouse.x + offset.0,
                        y: mouse.y + offset.1,
                    };
                    let _: () = msg_send![window, setFrameOrigin: o];
                    // 方向按水平位移判定，变化时上报一次
                    let d: i8 = if mouse.x < press.0 { -1 } else { 1 };
                    if *dir != Some(d) {
                        *dir = Some(d);
                        if let Some(a) = app.as_ref() {
                            let _ = a.emit("pet-drag", if d < 0 { "left" } else { "right" });
                        }
                    }
                } else if *armed {
                    // 已按下但尚未超过阈值：位移 >3px 才视为真拖拽（否则是单击）
                    let dx = mouse.x - press.0;
                    let dy = mouse.y - press.1;
                    if dx * dx + dy * dy > 9.0 {
                        *active = true;
                        let o = NSPoint {
                            x: mouse.x + offset.0,
                            y: mouse.y + offset.1,
                        };
                        let _: () = msg_send![window, setFrameOrigin: o];
                        let d: i8 = if dx < 0.0 { -1 } else { 1 };
                        *dir = Some(d);
                        if let Some(a) = app.as_ref() {
                            let _ = a.emit("pet-drag-start", true);
                            let _ = a.emit("pet-drag", if d < 0 { "left" } else { "right" });
                        }
                    }
                } else if over {
                    // 在可交互区域按下：记录偏移与按下位置，进入 armed 等待移动
                    let f: NSRect = msg_send![window, frame];
                    *offset = (f.origin.x - mouse.x, f.origin.y - mouse.y);
                    *press = (mouse.x, mouse.y);
                    *armed = true;
                    *dir = None;
                }
            } else if *active || *armed {
                // 松开左键：结束拖拽
                if *active {
                    if let Some(a) = app.as_ref() {
                        let _ = a.emit("pet-drag-end", true);
                    }
                }
                *active = false;
                *armed = false;
                *dir = None;
            }
        }
        Some(())
    }

    // 定义一个 NSObject 子类作为 NSTimer 的 target，带一个 tick 方法。
    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "PetHitTestTimerTarget"]
        struct PetTimerTarget;

        impl PetTimerTarget {
            // NSTimer 回调：每 16ms 在主线程执行一次（穿透 + hover + drag 共用）。
            #[unsafe(method(tick:))]
            fn tick(&self, _timer: *mut AnyObject) {
                // ⚠️ NSTimer 回调运行在 ObjC 栈帧上，panic 无法安全 unwind 过去，
                // 会直接 abort 整个进程。而 tick 每 16ms 执行一次，
                // 一旦某次 panic 会让 App 变成「必崩」状态，故整体包 catch_unwind。
                //
                // AssertUnwindSafe 是必需的：闭包捕获了 &self，编译器无法证明其
                // unwind 安全性；这里的状态都是 Mutex 包裹的独立静态量，
                // panic 后不会留下不一致的可观察状态。
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    tick_inner();
                }));
                if result.is_err() {
                    eprintln!("[macos_pet] tick 发生 panic，已捕获以避免进程 abort");
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

        {
            let mut h = APP_HANDLE.lock().unwrap();
            *h = Some(app.clone());
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
            let window = ns_window_ptr as *mut AnyObject;

            // ── 1) 核心：非激活窗口。覆盖 canBecomeMainWindow → NO（仅宠物窗口）──
            // 用 class_replaceMethod 覆盖现有类的该方法（而非 object_setClass 换类），
            // 保留 tao 的 focusable ivar 与 sendEvent: 原生拖拽逻辑。
            //
            // 风险：class_replaceMethod 直接 hook tao 创建的 NSWindow 子类(未文档化内部类),
            // macOS 大版本升级可能改变其类结构,导致 hook 失效或行为异常。因此:
            //   (a) 整个 hook 包在 catch_unwind 中,任一 objc 调用 panic 都不会拖垮 app;
            //   (b) hook 后立刻读回 canBecomeMainWindow 验证是否真生效,失败则降级。
            let hook_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let cls: *mut AnyClass = msg_send![window, class];
                let sel = sel!(canBecomeMainWindow);
                let types: *const c_char = {
                    let m: *const Method = class_getInstanceMethod(cls as *const AnyClass, sel);
                    if m.is_null() {
                        c"c@:".as_ptr()
                    } else {
                        method_getTypeEncoding(m)
                    }
                };
                let imp: Imp = std::mem::transmute::<
                    unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool,
                    Imp,
                >(pet_can_become_main_window);
                let _ = class_replaceMethod(cls, sel, imp, types);
            }))
            .is_ok();

            if !hook_ok {
                eprintln!(
                    "[macos_pet] 警告: 覆盖 canBecomeMainWindow 失败(hook panic)。\
                     降级为「整窗可交互、不拦截激活」,宠物仍可显示但可能抢焦点。"
                );
            } else {
                // 自检:hook 后读回 canBecomeMainWindow,确认返回 NO(非激活)。
                // 若返回 YES,说明 hook 未真正生效(类结构变化等),降级提示。
                let ok: Bool = msg_send![window, canBecomeMainWindow];
                if ok != Bool::NO {
                    eprintln!(
                        "[macos_pet] 警告: canBecomeMainWindow 自检返回 YES(hook 未生效)。\
                         降级为「整窗可交互」,宠物可能抢占其它 App 焦点。"
                    );
                }
            }

            // ── 2) 非激活状态下也能接收鼠标移动事件 ──
            let _: () = msg_send![window, setAcceptsMouseMovedEvents: true];

            // ── 3) 全 Space 一致表现：避免切换虚拟桌面时窗口被系统用不同方式
            //    重新合成/重绘，导致边框状态不一致而闪烁。
            //    之前这里是「mask | UtilityWindow | NonactivatingPanel」再 setStyleMask，
            //    已删除：NonactivatingPanel 这个 bit 只对真正的 NSPanel 子类生效，
            //    普通 NSWindow 加了没用；UtilityWindow 会让 AppKit 尝试画一层系统级
            //    「工具面板」描边，在某些 Space 重新合成窗口时这层描边闪烁地重绘，
            //    正是「个别桌面边框闪烁」的根因。
            //
            // NSWindowCollectionBehavior：
            //   CanJoinAllSpaces (1<<0=1) | Stationary (1<<4=16)
            //   | IgnoresCycle (1<<6=64) | FullScreenAuxiliary (1<<8=256) = 337
            // 让宠物窗口在任何 Space / 全屏其它 App 时都以同一种方式常驻显示，
            // 不参与 Cmd+Tab 循环、不随 Space 切换被重新分配/重绘。
            let _: () = msg_send![window, setCollectionBehavior: 337u64];

            // 彻底禁止 AppKit 自己移动这个窗口（无论是「拖背景移动窗口」还是任何其它系统级
            // 拖动路径）——窗口位置 100% 只由我们自己 NSTimer 里的 setFrameOrigin 决定。
            // setMovable:NO 只禁止「用户发起的系统级拖动」，不影响程序自己调用 setFrameOrigin:，
            // 所以不会影响我们自己的拖拽逻辑。
            let _: () = msg_send![window, setMovable: false];
            let _: () = msg_send![window, setMovableByWindowBackground: false];

            let clear: *mut AnyObject = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![window, setBackgroundColor: clear];
            let _: () = msg_send![window, setOpaque: false];
            let _: () = msg_send![window, setHasShadow: false];
            let _: () = msg_send![window, setLevel: 3isize]; // NSFloatingWindowLevel（置顶）
            let _: () = msg_send![window, setAcceptsMouseMovedEvents: true];

            // 初始状态：可交互（false = 不忽略鼠标）
            let _: () = msg_send![window, setIgnoresMouseEvents: false];

            // 创建 target 实例（NSObject 子类，作为 NSTimer 的 target）。
            // 注意：define_class! 生成的类需先调用 class() 触发注册到 runtime，
            // 否则 class!(PetTimerTarget) 会因「class not found」panic。
            let cls = PetTimerTarget::class();
            let target: *mut AnyObject = msg_send![cls, alloc];
            let target: *mut AnyObject = msg_send![target, init];
            // scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:
            // 由主线程 RunLoop 持有，自动重复触发，无需手动持有 timer。
            // 16ms（约 60fps）：hover 与 drag 都要即时跟手。
            let _timer: *mut AnyObject = msg_send![
                class!(NSTimer),
                scheduledTimerWithTimeInterval: 0.016f64,
                target: &*target,
                selector: sel!(tick:),
                userInfo: std::ptr::null::<AnyObject>(),
                repeats: true
            ];
        }
        true
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lock_ok_returns_guard_when_unpoisoned() {
            let m: Mutex<i32> = Mutex::new(7);
            let g = lock_ok(&m).expect("未中毒时应正常取锁");
            assert_eq!(*g, 7);
        }

        #[test]
        fn lock_ok_returns_none_when_poisoned() {
            // 在持有锁期间 panic 使 Mutex 中毒；catch_unwind 只阻止进程 abort，
            // 不会清除中毒标志。lock_ok 必须返回 None 而非向上 panic。
            let m: Mutex<i32> = Mutex::new(0);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _g = m.lock().unwrap();
                panic!("poison");
            }));
            assert!(lock_ok(&m).is_none());
        }
    }
}

/// 对外入口：宠物窗口(main)创建后调用。
/// 仅在 macOS 编译（Windows 走 windows_pet 模块，main.rs 的 mac 分支才调用本函数）。
#[cfg(target_os = "macos")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    let _ = macos_impl::install(app);
}
