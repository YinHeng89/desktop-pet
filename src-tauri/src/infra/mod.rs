// 共享基础设施层：HTTP 客户端、HTTP 服务、存储、压缩归档等。
// 零 Tauri 依赖，全部可单测；被命令层与 CLI 复用。
pub mod http_client;
