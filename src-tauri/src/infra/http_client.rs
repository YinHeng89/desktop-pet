// 共享 HTTP 客户端基础设施（从 pet_import.rs 抽出，★ 零 Tauri 依赖，可单测）。
//
// 统一 UA 与超时，复用连接池（OnceLock）。后续 browse_online_pets / download_online_pet
// 统一从这里取客户端，避免每次浏览/下载都重建 TLS 上下文与连接池。

use std::time::Duration;

/// 连接建立超时：DNS/TLS 握手卡住时快速失败，避免画廊一直转圈。
pub const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 整体请求超时：必须设置——`reqwest` 默认无超时，网络挂起时 Promise 永不 resolve，
/// 前端画廊会一直卡在 loading，用户只能杀进程。
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
pub const HTTP_USER_AGENT: &str = "PetBuddy/0.1 (online-gallery)";

static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

/// 获取共享 HTTP 客户端：统一 UA 与超时，并复用连接池。
pub fn http_client() -> Result<&'static reqwest::Client, String> {
    if let Some(c) = HTTP_CLIENT.get() {
        return Ok(c);
    }
    let client = reqwest::Client::builder()
        .user_agent(HTTP_USER_AGENT)
        .connect_timeout(HTTP_CONNECT_TIMEOUT)
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;
    // 并发下可能已被其它线程写入；插入失败无害，取已有值即可
    let _ = HTTP_CLIENT.set(client);
    HTTP_CLIENT
        .get()
        .ok_or_else(|| "HTTP 客户端未初始化".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_client_is_reused_across_calls() {
        // 验证 OnceLock 生效：不是每次调用都新建客户端（否则连接池形同虚设）。
        // 超时配置无法从 reqwest::Client 读回断言，故只验证复用这一条可观测性质。
        let a = http_client().unwrap();
        let b = http_client().unwrap();
        assert!(std::ptr::eq(a, b));
    }
}
