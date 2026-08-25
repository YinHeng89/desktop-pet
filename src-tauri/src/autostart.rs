//! macOS 开机自启：通过 LaunchAgent（launchd）实现。
//!
//! 相比 tauri-plugin-autostart 的 LaunchAgent 封装，这里自实现以：
//!   1. 统一 plist 文件名（app_name = 产品名 "PetBuddy"，避免历史遗留的大小写不一致）；
//!   2. 可控的路径逻辑（打包后指向 .app/MacOS 可执行文件）；
//!   3. 提供 is_enabled 精确查询（按 Label 判断）。
//!
//! plist 位置：~/Library/LaunchAgents/PetBuddy.plist

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// plist 文件名（Label + 文件名，统一用产品名，避免大小写不一致导致重复注册）。
const APP_NAME: &str = "PetBuddy";

/// 获取当前可执行文件路径。
/// - 打包后：/Applications/PetBuddy.app/Contents/MacOS/petbuddy
/// - dev 模式：src-tauri/target/debug/petbuddy
///
/// LaunchAgent 的 ProgramArguments 直接指向该可执行文件即可（launchd 可直接运行
/// .app/MacOS 下的二进制；它不要求必须指向 .app bundle）。
fn current_exe_path() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("获取可执行文件路径失败: {e}"))?;
    let exe = exe
        .canonicalize()
        .unwrap_or(exe)
        .to_string_lossy()
        .to_string();
    Ok(exe)
}

fn plist_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法获取用户主目录".to_string())?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{APP_NAME}.plist")))
}

/// 注册开机自启：写入 plist 文件（RunAtLoad=true）。
///
/// 注意：不主动 launchctl bootstrap 加载。macOS 的 LaunchAgent 在用户登录时会
/// 自动扫描 ~/Library/LaunchAgents/ 并加载 RunAtLoad=true 的服务。
/// 若在此处 bootstrap，会因 RunAtLoad 导致「点开关立即 spawn 一个新进程」，
/// 且 dev 模式下该进程缺少 GUI 会话上下文会立即退出，体验很差。
pub fn enable() -> Result<(), String> {
    let exe = current_exe_path()?;
    let path = plist_path()?;

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建 LaunchAgents 目录失败: {e}"))?;
    }

    let data = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
           <dict>\n\
             <key>Label</key>\n\
             <string>{APP_NAME}</string>\n\
             <key>ProgramArguments</key>\n\
             <array>\n\
               <string>{exe}</string>\n\
             </array>\n\
             <key>RunAtLoad</key>\n\
             <true/>\n\
           </dict>\n\
         </plist>\n"
    );
    fs::write(&path, data).map_err(|e| format!("写入 plist 失败: {e}"))?;

    Ok(())
}

/// 取消开机自启：launchctl 卸载（若已加载）+ 删除 plist 文件。
pub fn disable() -> Result<(), String> {
    let path = plist_path()?;
    launchctl_bootout(&path);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除 plist 失败: {e}"))?;
    }
    Ok(())
}

/// 通过 launchctl bootout 卸载服务（幂等，不存在时静默忽略）。
fn launchctl_bootout(plist: &PathBuf) {
    if let Ok(uid) = get_uid() {
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}")])
            .arg(plist)
            .output();
    }
}

/// 获取当前用户的 uid（用于 launchctl gui/<uid> 域）。
fn get_uid() -> Result<u32, String> {
    // 优先用 id -u（简单可靠），失败则回退到 getuid
    if let Ok(out) = Command::new("id").arg("-u").output() {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                if let Ok(uid) = s.trim().parse::<u32>() {
                    return Ok(uid);
                }
            }
        }
    }
    Err("获取 uid 失败".into())
}

/// 查询是否已注册开机自启（按 plist 文件是否存在判断）。
pub fn is_enabled() -> Result<bool, String> {
    Ok(plist_path()?.exists())
}
