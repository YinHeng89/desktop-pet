// 在线画廊（browse / download）的纯映射逻辑。
//
// 本模块零平台依赖（不碰 tauri / 网络 / 文件系统），所有函数可 100% 单测。
// 原逻辑来自 pet_import.rs 的 `browse_online_pets` 与 URL 拼接，抽纯后行为对齐。

pub mod index;
