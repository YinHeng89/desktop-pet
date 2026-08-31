//! 统一错误类型。
//!
//! 领域层纯函数统一返回 `Result<T, AppError>`；命令层目前仍以 `String` 为对外错误
//! 类型（保持 Tauri IPC 兼容），在调用点把 `AppError` 映射为可读文案即可。
//! 后续 Phase 可将命令签名整体切换到 `Result<T, AppError>`，并把 `ErrorCode`
//! 通过 ts-rs 生成到前端做精确匹配。

use std::path::PathBuf;

/// 错误码（与前端 `shared/errors/messages.ts` 一一对应，靠契约测试对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidPetId,
    ZipSlip,
    TooLarge,
    BadRequest,
    Network,
    Io,
    Platform,
    Serialization,
}

impl ErrorCode {
    /// 稳定字符串标识，便于序列化 / 日志 / 前端匹配。
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidPetId => "invalid_pet_id",
            ErrorCode::ZipSlip => "zip_slip",
            ErrorCode::TooLarge => "too_large",
            ErrorCode::BadRequest => "bad_request",
            ErrorCode::Network => "network",
            ErrorCode::Io => "io",
            ErrorCode::Platform => "platform",
            ErrorCode::Serialization => "serialization",
        }
    }
}

/// 应用错误：错误码 + 人类可读消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 宠物 id 非法（空或含非白名单字符）。
    pub fn invalid_pet_id(id: &str) -> Self {
        AppError::new(ErrorCode::InvalidPetId, format!("宠物 id 含非法字符或为空: {id}"))
    }

    /// 路径逃逸出允许目录（目录穿越）。
    pub fn zip_slip(path: &PathBuf) -> Self {
        AppError::new(ErrorCode::ZipSlip, format!("路径逃逸出允许目录: {path:?}"))
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::new(ErrorCode::Io, e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::new(ErrorCode::Serialization, e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::new(ErrorCode::Network, e.to_string())
    }
}
