//! 开机自启。
//!
//! - macOS：使用 Apple 官方 `SMAppService.mainApp`（登录项，macOS 13+）。
//!   注册后应用会出现在「系统设置 → 通用 → 登录项」中，用户可直观管理。
//!   相比手写 LaunchAgent plist，这是现代 macOS 官方推荐方案，更稳定、可被系统管理。
//!
//!   注意：SMAppService 要求应用是已打包的 .app（位于 /Applications 或有效 bundle），
//!   dev 模式（target/debug 裸二进制）无法注册。
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

#[cfg(not(target_os = "macos"))]
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
