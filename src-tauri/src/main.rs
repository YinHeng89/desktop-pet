#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod macos_pet;
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

/// 控制 macOS Dock 图标是否显示。
/// 打开设置窗口时设为 Regular（显示 Dock）；关闭设置窗口时设为 Accessory（隐藏 Dock）。
/// 与宠物是否显示无关。仅在 macOS 主线程调用有效。
#[cfg(target_os = "macos")]
fn set_dock_visible(visible: bool) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2::MainThreadMarker;
    // setActivationPolicy 必须在主线程调用
    let mtm = match MainThreadMarker::new() {
        Some(mtm) => mtm,
        None => {
            eprintln!("[dock] 非主线程，跳过切换 Dock 图标");
            return;
        }
    };
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    let app = NSApplication::sharedApplication(mtm);
    if !app.setActivationPolicy(policy) {
        eprintln!("[dock] 切换激活策略失败 visible={visible}");
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
/// 注意：只调整尺寸，不重新定位。窗口位置由用户拖动决定，
/// 缩放时若强制锚定右下角会导致宠物位置被重置。
#[tauri::command]
fn resize_pet_window(app: tauri::AppHandle, scale: f64) {
    let Some(w) = app.get_webview_window("main") else { return };
    let scale = scale.clamp(0.8, 1.3);

    let pet_w = (192.0 * scale).round();
    let pet_h = (208.0 * scale).round();
    let bubble_h = (96.0 * scale).round();
    // 宽：基线 320 × scale（等比缩放，与 worktrack 一致）；
    // 高：气泡区 + 宠物区 + 底部留白 16
    let ww = pet_w.max(320.0 * scale);
    let wh = bubble_h + pet_h + 16.0;

    let _ = w.set_size(tauri::LogicalSize::new(ww, wh));
}

/// 打开设置窗口（居中显示并聚焦；已存在则复用）。
#[tauri::command]
fn open_settings_window(app: tauri::AppHandle) {
    // 打开设置时显示 Dock 图标
    #[cfg(target_os = "macos")]
    set_dock_visible(true);
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.center();
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 隐藏设置窗口（前端关闭按钮调用）。
#[tauri::command]
fn close_settings_window(app: tauri::AppHandle) {
    // 关闭设置时隐藏 Dock 图标
    #[cfg(target_os = "macos")]
    set_dock_visible(false);
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
                    // 紧凑宠物窗口：320×320（气泡在上 + 宠物在下），定位屏幕右下角（避 Dock）。
                    if let Ok(monitor) = w.current_monitor() {
                        if let Some(mon) = monitor {
                            let size = mon.size();
                            let scale = mon.scale_factor();
                            let ww = 320.0;
                            let wh = 320.0;
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
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                    // 关闭设置时隐藏 Dock 图标（原生关闭按钮 / Cmd+W 路径）
                    #[cfg(target_os = "macos")]
                    set_dock_visible(false);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            quit_app,
            set_autostart,
            get_autostart,
            broadcast_event,
            resize_pet_window,
            open_settings_window,
            close_settings_window,
            macos_pet::set_notify_interactive_rects,
            pet_import::import_pet,
            pet_import::list_imported_pets,
            pet_import::delete_imported_pet,
            pet_import::browse_online_pets,
            pet_import::download_online_pet,
            notify_server::push_notify
        ])
        .run(tauri::generate_context!())
        .expect("error while running PetBuddy");
}
