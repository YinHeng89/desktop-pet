#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod macos_pet;
mod windows_pet;
mod pet_import;
mod notify_server;

use tauri::{Emitter, Listener, Manager};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, Submenu};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::WindowEvent;

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0)
}

/// 用系统默认程序（浏览器）打开外部链接。
#[tauri::command]
fn open_external(url: String) {
    if let Err(e) = open::that(&url) {
        eprintln!("[open_external] 打开链接失败: {url}: {e}");
    }
}

/// 设置开机自启（true=开启，false=关闭）。
#[tauri::command]
fn set_autostart(enabled: bool) -> Result<String, String> {
    if enabled {
        autostart::enable()?;
        Ok("enabled".into())
    } else {
        autostart::disable()?;
        Ok("disabled".into())
    }
}

/// 查询开机自启是否已开启。
#[tauri::command]
fn get_autostart() -> Result<bool, String> {
    autostart::is_enabled()
}

/// 检测本次启动是否来自开机自启（注册表 Run 键写入时附加了 --autostart 参数）。
#[tauri::command]
fn was_auto_started() -> bool {
    std::env::args().any(|a| a == "--autostart")
}

/// 前端通用跨窗口广播：invoke 后由 Rust app.emit 广播到所有窗口。
/// 用于 pet-switch / pet-scale / pet-visible 等前端→前端事件，
/// 规避前端 emit 跨窗口不生效的问题（走 IPC 更可靠）。
#[tauri::command]
fn broadcast_event(app: tauri::AppHandle, event: String, payload: Option<serde_json::Value>) {
    let _ = app.emit(&event, payload);
}

/// 按宠物缩放比例重设 main 窗口尺寸。
/// 缩放变化时调用：窗口跟随宠物一起缩放，保证气泡+宠物不被裁剪。
/// scale 范围 0.8~1.3；宠物帧 192×208，窗口 = 气泡区 + 宠物区 + 留白。
///
/// 按 scale 计算 main 宠物窗口的逻辑尺寸（宽×高，含 24px 缓冲）。
/// scale 范围与前端 MIN_SCALE(0.5)~MAX_SCALE(1.3) 对齐，避免 Rust 与前端口径分裂
/// 导致窗口尺寸和精灵图渲染尺寸不一致（宠物浮在窗口偏左上、右下角留白）。
fn pet_window_size(scale: f64) -> (f64, f64) {
    let scale = scale.clamp(0.5, 1.3);
    let pet_w = (192.0 * scale).round();
    let pet_h = (208.0 * scale).round();
    let bubble_h = (156.0 * scale).round();
    // 宽：基线 320 × scale（等比缩放，与 worktrack 一致）；
    // 高：气泡区 + 宠物区 + 底部留白 16（scale=1 时 156+208+16=380）
    let mut ww = pet_w.max(320.0 * scale);
    let mut wh = bubble_h + pet_h + 16.0;
    // 缓冲：窗口比内容大一圈，避免缩放过程中宠物（canvas 已先按新 scale 渲染）
    // 短暂超出旧窗口被 OS 裁掉而闪。宠物贴窗口 right/bottom:16px，缓冲落在
    // 左/上透明区，不影响视觉锚点。
    let pad = 24.0;
    ww += pad;
    wh += pad;
    (ww, wh)
}

/// 缩放时以窗口【右下角】为锚点重新定位：宠物贴窗口右下角，原地缩放不漂移。
#[tauri::command]
fn resize_pet_window(app: tauri::AppHandle, scale: f64) {
    let Some(w) = app.get_webview_window("main") else { return };
    let scale = scale.clamp(0.5, 1.3);

    let (ww, wh) = pet_window_size(scale);

    // 关键时序：先读出【当前】位置与尺寸（旧值），再 set_size，最后用旧值算锚点
    // set_position。不能在 set_size 之后才 outer_size()——那时 OS 可能已改为新尺寸，
    // 旧位置 + 新宽度混用会让右下角算错，宠物每帧跳一下。
    let sf = w.scale_factor().unwrap_or(1.0);
    if let (Ok(pos), Ok(old)) = (w.outer_position(), w.outer_size()) {
        // 旧右下角屏幕坐标（物理像素）：调用前的位置 + 调用前的尺寸
        let right = pos.x + old.width as i32;
        let bottom = pos.y + old.height as i32;
        // 先改尺寸，再按「旧右下角」把新左上角定位回去，保证右下角不动
        let _ = w.set_size(tauri::LogicalSize::new(ww, wh));
        let _ = w.set_position(tauri::PhysicalPosition::new(
            ((right as f64 - ww * sf).round()) as i32,
            ((bottom as f64 - wh * sf).round()) as i32,
        ));
    } else {
        // 读不到旧状态（极端情况）时退化为仅改尺寸，避免窗口卡死
        let _ = w.set_size(tauri::LogicalSize::new(ww, wh));
    }
}

/// 设置窗口最后位置的持久化文件名（存于 app_data_dir）。
const SETTINGS_WIN_POS_FILE: &str = "petbuddy_settings_window.json";

/// 读取设置窗口上次保存的位置（逻辑像素）。无文件/解析失败返回 None。
fn load_settings_window_pos(app: &tauri::AppHandle) -> Option<(f64, f64)> {
    let dir = app.path().app_data_dir().ok()?;
    let path = dir.join(SETTINGS_WIN_POS_FILE);
    let content = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let x = v.get("x")?.as_f64()?;
    let y = v.get("y")?.as_f64()?;
    Some((x, y))
}

/// 保存设置窗口当前位置（逻辑像素）。失败仅打印日志，不影响主流程。
fn save_settings_window_pos(app: &tauri::AppHandle, x: f64, y: f64) {
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[settings-pos] 取 app_data_dir 失败: {e}");
            return;
        }
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(SETTINGS_WIN_POS_FILE);
    let json = serde_json::json!({ "x": x, "y": y });
    if let Ok(s) = serde_json::to_string(&json) {
        let _ = std::fs::write(path, s);
    }
}

/// 打开设置窗口（首次居中，之后恢复到上次关闭前的位置；已存在则复用）。
#[tauri::command]
fn open_settings_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        // 仅当从未保存过位置时才居中（固定首次默认位置）；
        // 之后都恢复到用户拖动后的最后位置，避免每次打开都被重置到屏幕中心。
        if load_settings_window_pos(&app).is_none() {
            let _ = w.center();
        }
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 隐藏设置窗口（前端关闭按钮调用）。
#[tauri::command]
fn close_settings_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.hide();
    }
}

/// 构建托盘菜单（PetBuddy 无登录态，菜单固定）
fn build_tray_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open_settings = MenuItem::with_id(app, "open_settings", "打开设置", true, None::<&str>)?;
    let toggle_visible = MenuItem::with_id(app, "toggle_visible", "显示/隐藏宠物", true, None::<&str>)?;
    // 开机自启用 CheckMenuItem：勾选状态反映当前是否已开启
    let autostart_enabled = autostart::is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "toggle_autostart",
        "开机自启",
        true,
        autostart_enabled,
        None::<&str>,
    )?;

    // 「切换宠物」子菜单：内置 + 外部动态扫描
    let mut pet_items: Vec<MenuItem<tauri::Wry>> = vec![
        MenuItem::with_id(app, "pet:miku", "Miku", true, None::<&str>)?,
        MenuItem::with_id(app, "pet:ryujinmaru", "龙神丸", true, None::<&str>)?,
        MenuItem::with_id(app, "pet:Seedy", "Seedy", true, None::<&str>)?,
    ];
    for (ext_id, ext_name) in pet_import::list_imported_pet_meta(app) {
        let menu_id = format!("pet:{}", ext_id);
        pet_items.push(MenuItem::with_id(app, menu_id.as_str(), ext_name.as_str(), true, None::<&str>)?);
    }
    pet_items.push(MenuItem::with_id(app, "pet:more", "更多设置…", true, None::<&str>)?);
    let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        pet_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>).collect();
    let pet_menu = Submenu::with_items(app, "切换宠物", true, &refs)?;

    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        vec![&open_settings, &toggle_visible, &autostart, &pet_menu, &quit];
    Menu::with_items(app, &items)
}

/// 重建托盘菜单（导入/删除外部宠物后调用，让「切换宠物」子菜单反映最新列表）。
fn rebuild_tray_menu(app: &tauri::AppHandle) {
    let menu = match build_tray_menu(app) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[tray] 重建菜单失败: {e}");
            return;
        }
    };
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_menu(Some(menu));
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // macOS：安装像素级鼠标穿透（宠物/气泡可交互，透明区域穿透）
            #[cfg(target_os = "macos")]
            {
                if let Some(w) = app.get_webview_window("main") {
                    // 紧凑宠物窗口（气泡在上 + 宠物在下），定位屏幕右下角（避 Dock）。
                    // 初始尺寸用语公式（scale=0.7，与前端 DEFAULT_SCALE 一致），
                    // 避免 setup 用裸 320×380 而前端 onMounted 再 resize 导致首屏跳动。
                    if let Ok(monitor) = w.current_monitor() {
                        if let Some(mon) = monitor {
                            let size = mon.size();
                            let scale = mon.scale_factor();
                            let (ww, wh) = pet_window_size(0.7);
                            // 逻辑坐标：右边距 24、底边距 75（避开 Dock）
                            let x = (size.width as f64 / scale) - ww - 24.0;
                            let y = (size.height as f64 / scale) - wh - 75.0;
                            let _ = w.set_size(tauri::LogicalSize::new(ww, wh));
                            let _ = w.set_position(tauri::LogicalPosition::new(x, y));
                        }
                    }
                    // 安装穿透（作用于 main 宠物窗口）
                    macos_pet::setup_notify_interactive(app.handle());
                    let _ = w.show();
                }
            }

            // Windows：安装透明区域鼠标穿透（SetWindowRgn 把窗口裁成可交互矩形）。
            // 与 macOS 的 NSTimer 动态切换不同，Windows 用静态区域裁切，
            // 由前端上报 rect 后调用 apply_pet_hit_rects 即时生效。
            #[cfg(target_os = "windows")]
            {
                if let Some(w) = app.get_webview_window("main") {
                    // 初始定位：钉到屏幕右下角，避开任务栏。
                    // Windows 无 Dock，但有底部任务栏（Win11 约 48px 高），
                    // 用 64px 底边距 + 24px 右边距兜底，避免宠物被任务栏遮住。
                    // 初始尺寸用语公式（scale=0.7，与前端 DEFAULT_SCALE 一致），
                    // 避免 setup 用裸 320×380 而前端 onMounted 再 resize 导致首屏跳动。
                    if let Ok(monitor) = w.current_monitor() {
                        if let Some(mon) = monitor {
                            let size = mon.size();
                            let scale = mon.scale_factor();
                            let (ww, wh) = pet_window_size(0.7);
                            let x = (size.width as f64 / scale) - ww - 24.0;
                            let y = (size.height as f64 / scale) - wh - 64.0;
                            let _ = w.set_size(tauri::LogicalSize::new(ww, wh));
                            let _ = w.set_position(tauri::LogicalPosition::new(x, y));
                        }
                    }
                    windows_pet::setup_notify_interactive(app.handle());
                    // 定位完成后再显示，避免先以默认位置闪现一帧再瞬移右下角。
                    let _ = w.show();
                    // show 之后立即按默认 scale(0.7) 的精确尺寸校正窗口，规避 visible:false 下
                    // WebView2 surface 首次初始化尺寸错误导致的宠物右侧/下方被裁剪。
                    // 定位已在 show 前完成，此处仅校正尺寸（右下角锚点保持不变），
                    // 且与前端 onMounted 的 resizePetWindow(默认scale) 口径一致，不再二次跳动。
                    resize_pet_window(app.handle().clone(), 0.7);
                }

                // settings 窗口：透明无边框窗口在 Windows 上仅靠 CSS border-radius 无法
                // 真正裁切窗口边角，需要调用 DWM 设置系统级圆角（Windows 11）。
                if let Some(w) = app.get_webview_window("settings") {
                    if let Ok(hwnd) = w.hwnd() {
                        windows_pet::setup_window_rounded_corners(hwnd.0 as isize);
                    }
                }
            }

            // 托盘
            let menu = build_tray_menu(app.handle())?;
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("PetBuddy")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        let _ = app.exit(0);
                    }
                    "open_settings" => {
                        open_settings_window(app.clone());
                    }
                    "toggle_visible" => {
                        let _ = app.emit("pet-toggle-visible", ());
                    }
                    "toggle_autostart" => {
                        // 直接在 Rust 侧切换自启状态，并重建托盘菜单（刷新勾选状态）
                        let current = autostart::is_enabled().unwrap_or(false);
                        let result = if current {
                            autostart::disable()
                        } else {
                            autostart::enable()
                        };
                        if let Err(e) = result {
                            eprintln!("[autostart] 切换失败: {e}");
                        }
                        rebuild_tray_menu(app);
                    }
                    id if id.starts_with("pet:") => {
                        let _ = app.emit("pet-switch", id.to_string());
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                        let app = tray.app_handle();
                        open_settings_window(app.clone());
                    }
                })
                .build(app)?;

            // 启动本地通知 HTTP 服务（供外部应用调用发通知）
            notify_server::start(app.handle().clone());

            // 监听「宠物列表变化」事件：导入/删除外部宠物后重建托盘菜单。
            // listen_any 返回 EventId（值类型句柄），监听器本身存在全局 manager 中，随 app 存活。
            {
                let handle = app.handle().clone();
                let handle_for_cb = handle.clone();
                let _ = handle.listen_any("pet-pets-changed", move |_event| {
                    rebuild_tray_menu(&handle_for_cb);
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // settings 窗口关闭时隐藏而非销毁，便于下次复用打开
            if window.label() == "settings" {
                match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    // 用户拖动设置窗口时实时保存最后位置，下次打开恢复（而非重置居中）
                    WindowEvent::Moved(pos) => {
                        save_settings_window_pos(window.app_handle(), pos.x.into(), pos.y.into());
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            quit_app,
            open_external,
            set_autostart,
            get_autostart,
            was_auto_started,
            broadcast_event,
            resize_pet_window,
            open_settings_window,
            close_settings_window,
            macos_pet::set_notify_interactive_rects,
            windows_pet::set_pet_hit_rects,
            windows_pet::apply_pet_hit_rects,
            windows_pet::hide_pet_window,
            pet_import::import_pet,
            pet_import::list_imported_pets,
            pet_import::delete_imported_pet,
            pet_import::update_imported_pet,
            pet_import::browse_online_pets,
            pet_import::download_online_pet,
            notify_server::push_notify
        ])
        .run(tauri::generate_context!())
        .expect("error while running PetBuddy");
}
