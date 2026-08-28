// Windows 专用:桌面宠物(main 窗口)的可交互区域鼠标穿透。
// 思路与 macOS 的 macos_pet.rs(NSTimer + ignoresMouseEvents)对齐,但 Windows 用
// SetWindowRgn 把窗口裁成「仅宠物 + 气泡矩形可交互」,区域外点击穿透到下层桌面/窗口。
//
// 本文件在所有平台都参与编译(不能用 #![cfg(windows)] 包整个文件,否则 tauri 的
// generate_handler! 在 macOS 上找不到 command 符号)。真正的 Win32 API 调用用
// #[cfg(target_os = "windows")] 限定在函数内部,非 Windows 平台提供 no-op 实现。
//
// 注意 windows-sys 0.52 的类型约定:HWND / HRGN 等都是 isize 的 newtype(无 .is_invalid()
// 方法),判空用 `== 0`;SetWindowRgn 第三参数 bRedraw 是 BOOL(i32),传 1 而非 true。
//
// GDI 对象所有权规则:SetWindowRgn 调用**成功**后,region 的所有权转移给系统,
// 之后不能再对它调用 DeleteObject(会导致 double-free / 未定义行为)。
// 但如果 SetWindowRgn **失败**(返回 0),region 仍归调用者所有,必须自己 DeleteObject,
// 否则泄漏 GDI 句柄。下面所有 SetWindowRgn 调用都据此检查返回值。
//
// DPI 缩放注意事项:GetDeviceCaps(hdc, LOGPIXELSX) 返回的是系统/主显示器 DPI,
// 不是"这个窗口当前所在屏幕"的 DPI——如果窗口被拖到跟主屏缩放比例不同的副屏上,
// 用 GetDeviceCaps 算出来的 scale 会是错的(命中矩形跟实际物理像素对不上)。
// 改用 GetDpiForWindow(hwnd),它是按窗口取值,会随窗口跨屏移动动态更新
// (前提是窗口声明了 Per-Monitor V2 DPI 感知,Tauri 默认会声明)。
// GetDeviceCaps 保留作为 GetDpiForWindow 返回 0 时的兜底路径。

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
const WINDOW_CORNER_RADIUS: i32 = 14; // 与气泡/设置窗圆角一致
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

/// 前端显式触发:把当前 hit rects 应用到 main 窗口(SetWindowRgn 即时生效)。
/// 非 Windows 平台为 no-op(macOS 走 macos_pet 的 NSTimer 方案)。
#[tauri::command]
pub fn apply_pet_hit_rects(app: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Some(w) = app.get_webview_window("main") {
            if let Ok(hwnd) = w.hwnd() {
                // tauri 的 HWND 是 windows_sys::Win32::Foundation::HWND(isize newtype)
                apply_hit_rects(hwnd.0 as isize);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

/// 隐藏 main 宠物窗口。
///
/// 修复要点(原代码顺序反了):原来是「先 SetWindowRgn(0) 清 region → 再 hide」,
/// 但 SetWindowRgn 会同步触发 DWM 立即按"整窗矩形、无裁切"重绘一帧,这一帧发生在
/// hide() 真正让窗口消失之前,于是用户会看到一闪而过的窗口边框/阴影。
///
/// 正确顺序:先 hide()(窗口已不可见,不会有画面暴露给用户),再清 region
/// (清掉裁切,避免下次 show 时残留旧的小 region 导致"缩略图"问题)。
/// 且清 region 这一步用 bRedraw=0(不重绘),因为窗口已经隐藏,不需要立即生效的重绘。
#[tauri::command]
pub fn hide_pet_window(app: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        if let Some(w) = app.get_webview_window("main") {
            // 1. 先隐藏:窗口不可见后,任何后续重绘都不会呈现给用户
            let _ = w.hide();

            // 2. 再清 region(HRGN=0 表示清除裁切),bRedraw=0 不强制重绘
            if let Ok(hwnd) = w.hwnd() {
                use windows_sys::Win32::Graphics::Gdi::SetWindowRgn;
                unsafe {
                    SetWindowRgn(hwnd.0 as isize, 0, 0);
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
    }
}

/// 对外入口:main 窗口创建后调用,安装 Windows 穿透。
/// 非 Windows 平台为 no-op(macOS 由 macos_pet::setup_notify_interactive 处理)。
#[cfg(target_os = "windows")]
pub fn setup_notify_interactive(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(hwnd) = w.hwnd() {
            // 初始先整窗可交互(未初始化前不裁切,避免误穿透)
            apply_hit_rects(hwnd.0 as isize);
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
/// 本项目把设置窗圆角定为 8px 而非 14px 的原因。宠物窗口因需要区域级穿透，仍用
/// `SetWindowRgn`(`apply_hit_rects`)，与本函数互不影响。
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

#[cfg(target_os = "windows")]
pub fn apply_hit_rects(hwnd: isize) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

    // 强制 DWM 立即重新合成窗口内容(含 WebView 子窗口)。
    // 慢机器/首帧场景下,仅 SetWindowRgn 的 bRedraw 只重画 frame 裁切区,
    // 不保证 DWM 把 WebView 位图按新 region 重绘——表现为「只渲染出局部,
    // 点一下(交互触发重绘)才恢复完整」。此处向窗口投递 WM_PAINT(0x000F)
    // 强制一次绘制,复现「点击后恢复完整」的重绘效果,消除首帧渲染不全。
    // 注:windows-sys 0.52 的 RedrawWindow/InvalidateRect/UpdateWindow 在本项目
    // feature 下未暴露,改用最基础的 SendMessageW + WM_PAINT 字面量,避免符号缺失。
    let force_redraw = || unsafe {
        const WM_PAINT: u32 = 0x000F;
        SendMessageW(hwnd, WM_PAINT, 0, 0);
    };

    // hwnd 为 0 视为无效
    if hwnd == 0 {
        return false;
    }

    // 按窗口当前所在屏幕取 DPI scale(而不是固定用主屏/系统 DPI)
    let scale = window_dpi_scale(hwnd);

    let rects = match HIT_RECTS.lock() {
        Ok(g) => g.clone(),
        Err(_) => Vec::new(),
    };
    let initialized = matches!(RECTS_INITIALIZED.lock(), Ok(g) if *g);

    // 未初始化或空矩形 → 整窗可交互(传 0 region 清除裁切)
    if !initialized || rects.is_empty() {
        unsafe {
            SetWindowRgn(hwnd, 0, 1);
        }
        force_redraw();
        return true;
    }

    // 复用跨平台纯函数把 CSS 逻辑矩形按 DPI scale 换算成物理像素矩形,
    // 保证与 macOS 端坐标换算口径一致(消除两份独立实现)。
    let physical = crate::geometry::rects_to_logical_physical(&rects, scale);

    // 为每个矩形生成带圆角的 HRGN 并合并(RGN_OR = 并集)。
    // HRGN 在 windows-sys 0.52 即 isize;0 表示空。
    let mut combined: isize = 0;
    let mut ok = false;
    for &(x, y, w, h) in &physical {
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let l = x.round() as i32;
        let t = y.round() as i32;
        let r = (x + w).round() as i32;
        let b = (y + h).round() as i32;
        let radius = (WINDOW_CORNER_RADIUS as f64 * scale).round() as i32;
        // CreateRoundRectRgn 最后两参数是椭圆宽/高(=直径)，非 CSS 半径；
        // 必须 ×2 才能与 border-radius:14px 视觉一致。
        let diameter = radius.saturating_mul(2);
        let rgn = unsafe { CreateRoundRectRgn(l, t, r, b, diameter, diameter) };
        if rgn == 0 {
            continue;
        }
        if combined == 0 {
            combined = rgn;
            ok = true;
        } else {
            unsafe {
                CombineRgn(combined, combined, rgn, RGN_OR);
                // 子区域合并后不再需要,销毁以释放 GDI 对象
                DeleteObject(rgn);
            }
        }
    }

    if ok && combined != 0 {
        // 检查 SetWindowRgn 返回值。
        // 成功(非 0):region 所有权转移给系统,不能再 DeleteObject。
        // 失败(0):region 仍归我们所有,必须自己 DeleteObject,否则泄漏 GDI 句柄。
        let result = unsafe { SetWindowRgn(hwnd, combined, 1) };
        if result == 0 {
            unsafe {
                DeleteObject(combined);
            }
            // 应用失败,回退为整窗可交互,避免用户完全无法交互
            unsafe {
                SetWindowRgn(hwnd, 0, 1);
            }
            force_redraw();
        } else {
            // 成功:region 所有权已转移给系统。强制 DWM 重绘,
            // 确保 WebView 内容按新裁切区域立即完整合成(消除首帧渲染不全)。
            force_redraw();
        }
        true
    } else {
        // 没有有效矩形:整窗可交互
        unsafe {
            SetWindowRgn(hwnd, 0, 1);
        }
        force_redraw();
        true
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