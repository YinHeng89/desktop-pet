//! 开机自启。
//!
//! - macOS：使用 Apple 官方 `SMAppService.mainApp`（登录项，macOS 13+）。
//!   注册后应用会出现在「系统设置 → 通用 → 登录项」中，用户可直观管理。
//!   相比手写 LaunchAgent plist，这是现代 macOS 官方推荐方案，更稳定、可被系统管理。
//!
//!   注意：SMAppService 要求应用是已打包的 .app（位于 /Applications 或有效 bundle），
//!   dev 模式（target/debug 裸二进制）无法注册。
//!
//! - Windows：在用户「启动」文件夹（`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`）
//!   写入 / 删除一个指向当前 exe 的 `.lnk` 快捷方式。这是 Windows 下最稳、零额外依赖
//!   的开机自启方案（无需注册表、无需 COM 提升权限）。`.lnk` 用纯二进制手写 Shell Link
//!   格式（最小可用子集：LinkTargetIDList + Unicode 字符串数据块），不依赖任何第三方库。
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
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    /// 启动项快捷方式文件名（位于用户 Startup 文件夹）。
    const LNK_NAME: &str = "PetBuddy.lnk";

    /// 用户「启动」文件夹下的 `.lnk` 完整路径。
    /// Windows 开机登录后会自动枚举该目录并执行其中所有快捷方式。
    fn startup_lnk_path() -> Option<PathBuf> {
        let appdata = env::var("APPDATA").ok()?;
        Some(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup")
                .join(LNK_NAME),
        )
    }

    pub fn enable() -> Result<(), String> {
        // 当前 exe 绝对路径（打包后为 .exe，dev 模式为 debug 二进制）
        let exe = env::current_exe().map_err(|e| format!("取当前 exe 路径失败: {e}"))?;
        let target = exe.to_string_lossy().to_string();
        // 工作目录：用当前进程目录，避免从 Startup 启动时 cwd 异常
        let cwd = env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let path = startup_lnk_path().ok_or("取 Startup 目录失败（APPDATA 未设置）")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建 Startup 目录失败: {e}"))?;
        }

        let bytes = build_lnk(&target, &cwd);
        fs::write(&path, bytes).map_err(|e| format!("写入快捷方式失败: {e}"))?;
        Ok(())
    }

    pub fn disable() -> Result<(), String> {
        if let Some(path) = startup_lnk_path() {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| format!("删除快捷方式失败: {e}"))?;
            }
        }
        Ok(())
    }

    pub fn is_enabled() -> Result<bool, String> {
        Ok(startup_lnk_path().map(|p| p.exists()).unwrap_or(false))
    }

    /// 手写最小可用 Shell Link（.lnk）二进制：
    /// ShellLinkHeader + LinkTargetIDList（最小 My Computer 根项）+ Unicode 字符串数据块。
    /// 目标路径 / 工作目录 / 参数全部走字符串数据块，无需 COM / IShellLink。
    fn build_lnk(target: &str, cwd: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(512);

        // ── ShellLinkHeader（76 字节）──
        out.extend_from_slice(&0x0000_004C_u32.to_le_bytes()); // HeaderSize = 76
        // LinkCLSID = {00021401-0000-0000-C000-000000000046}
        out.extend_from_slice(&[
            0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x46,
        ]);
        // LinkFlags：HasLinkTargetIDList | HasName | HasWorkingDir | HasArguments | IsUnicode
        let flags: u32 = 0x01 | 0x02 | 0x10 | 0x20 | 0x80;
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // FileAttributes
        out.extend_from_slice(&[0u8; 8]); // CreationTime
        out.extend_from_slice(&[0u8; 8]); // AccessTime
        out.extend_from_slice(&[0u8; 8]); // WriteTime
        out.extend_from_slice(&0u32.to_le_bytes()); // FileSize
        out.extend_from_slice(&0i32.to_le_bytes()); // IconIndex
        out.extend_from_slice(&1u32.to_le_bytes()); // ShowCommand = SW_SHOWNORMAL
        out.extend_from_slice(&0u16.to_le_bytes()); // HotKey
        out.extend_from_slice(&0u16.to_le_bytes()); // Reserved1
        out.extend_from_slice(&0u32.to_le_bytes()); // Reserved2
        out.extend_from_slice(&0u32.to_le_bytes()); // Reserved3

        // ── LinkTargetIDList（最小可用：My Computer 根项 + 终止符）──
        // IDListSize（后续 itemIDList 总字节数，含 2 字节终止符）= 20 + 2 = 22
        out.extend_from_slice(&0x0016_u16.to_le_bytes());
        // ItemID（size=20，data=18）：0x1F=根命名空间类型（My Computer）
        out.extend_from_slice(&[
            0x14, 0x00, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        out.extend_from_slice(&[0x00, 0x00]); // IDList 终止符（空 ItemID）

        // ── STRING_DATA（顺序：NAME → WORKING_DIR → ARGUMENTS）──
        push_unicode_string(&mut out, target); // NAME（HasName）= 目标 exe
        push_unicode_string(&mut out, cwd); // WORKING_DIR（HasWorkingDir）
        push_unicode_string(&mut out, ""); // ARGUMENTS（HasArguments）

        out
    }

    /// 写入一个 Unicode 字符串块：size(u16, 字符数含终止 null) + UTF-16LE 字节 + 终止 null。
    fn push_unicode_string(buf: &mut Vec<u8>, s: &str) {
        let utf16: Vec<u16> = s.encode_utf16().collect();
        let count = (utf16.len() + 1) as u16; // 含终止 null 字符
        buf.extend_from_slice(&count.to_le_bytes());
        for c in &utf16 {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        buf.extend_from_slice(&0u16.to_le_bytes());
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
