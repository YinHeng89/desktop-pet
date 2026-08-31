// PetBuddy 纯领域层。
//
// 本模块树**严禁**引入任何平台 API（tauri / objc2 / windows-sys / std::fs / std::net...）
// 也不依赖任何具体 IO，所有函数都是可 100% 单测的纯逻辑。平台相关的装配、
// IO 适配器、状态管理在 `crate` 根的其它模块里完成。
//
// 验收：`grep -rn "tauri" src/domain/` 必须为空。

pub mod gallery;
pub mod geometry;
pub mod layout;
pub mod notify;
pub mod pet;
