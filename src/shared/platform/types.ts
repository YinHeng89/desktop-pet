// 平台标识类型。值由 Rust `get_platform` 命令提供，前端不再用 UA 嗅探（R5）。

export type Platform = 'macos' | 'windows' | 'linux' | 'web' | 'unknown'
