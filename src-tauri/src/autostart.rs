//! 开机自启。
//!
//! - macOS：使用 Apple 官方 `SMAppService.mainApp`（登录项，macOS 13+）。
//!   注册后应用会出现在「系统设置 → 通用 → 登录项」中，用户可直观管理。
//!   相比手写 LaunchAgent plist，这是现代 macOS 官方推荐方案，更稳定、可被系统管理。
//!
//!   注意：SMAppService 要求应用是已打包的 .app（位于 /Applications 或有效 bundle），
//!   dev 模式（target/debug 裸二进制）无法注册。
//!
//! - Windows：通过 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 注册表键注册 /
//!   注销（currentUser 安装模式下 exe 位于用户可写目录，AB 更新不改变路径，Run 键只需
//!   首次注册一次即可永久生效）。注册值为当前 exe 完整路径 + `--autostart` 参数，
//!   供程序区分「开机自启」与「手动启动」。相比 Startup 文件夹 `.lnk` 方案，注册表方案
//!   更可靠（不会被误删、不依赖 Shell Link 解析），且与 worktrack 桌面端保持一致。
//!
//! - 其它平台：暂未实现（返回未启用）。

#[cfg(target_os = "macos")]
mod imp {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    /// 注册开机自启（登录项）。
    pub fn enable() -> Result<(), String> {
        // SMAppService::mainAppService() 对应「主应用作为登录项」
        let service = unsafe { SMAppService::mainAppService() };
        unsafe { service.registerAndReturnError() }
            .map_err(|e| format!("注册登录项失败: {}", e.localizedDescription()))?;
        Ok(())
    }

    /// 取消开机自启。
    pub fn disable() -> Result<(), String> {
        let service = unsafe { SMAppService::mainAppService() };
        unsafe { service.unregisterAndReturnError() }
            .map_err(|e| format!("取消登录项失败: {}", e.localizedDescription()))?;
        Ok(())
    }

    /// 查询是否已启用（Enabled 或 RequiresApproval 均视为「已注册」）。
    pub fn is_enabled() -> Result<bool, String> {
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };
        Ok(matches!(
            status,
            SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval
        ))
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use winreg::enums::*;
    use winreg::RegKey;

    /// 注册表 Run 键值名。
    const RUN_VALUE: &str = "PetBuddy";

    /// 注册开机自启（HKCU Run 键）。
    ///
    /// 写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\PetBuddy`
    /// 值为当前 exe 的完整路径（含引号）+ `--autostart` 标记。
    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| format!("获取 exe 路径失败: {e}"))?;
        let exe_str = exe
            .to_str()
            .ok_or_else(|| "exe 路径含非 UTF-8 字符".to_string())?;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run_key, _) = hkcu
            .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
            .map_err(|e| format!("打开 Run 键失败: {e}"))?;

        // 双引号包裹路径，附加 --autostart 标记供程序区分开机自启 / 手动启动
        let value = format!("\"{exe_str}\" --autostart");
        run_key
            .set_value(RUN_VALUE, &value)
            .map_err(|e| format!("写入 Run 键失败: {e}"))?;

        Ok(())
    }

    /// 取消开机自启（删除 Run 键值）。
    pub fn disable() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu
            .open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Run",
                KEY_READ | KEY_WRITE,
            )
            .map_err(|e| format!("打开 Run 键失败: {e}"))?;

        // 值不存在也视为成功（幂等）
        match run_key.delete_value(RUN_VALUE) {
            Ok(()) => Ok(()),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删除 Run 键失败: {e}")),
        }
    }

    /// 查询是否已注册开机自启。
    pub fn is_enabled() -> Result<bool, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu
            .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_READ)
            .map_err(|e| format!("打开 Run 键失败: {e}"))?;

        match run_key.get_value::<String, _>(RUN_VALUE) {
            Ok(_) => Ok(true),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(format!("读取 Run 键失败: {e}")),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    pub fn enable() -> Result<(), String> {
        Err("当前平台暂不支持开机自启".into())
    }
    pub fn disable() -> Result<(), String> {
        Err("当前平台暂不支持开机自启".into())
    }
    pub fn is_enabled() -> Result<bool, String> {
        Ok(false)
    }
}

pub use imp::{disable, enable, is_enabled};
