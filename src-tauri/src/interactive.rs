// 跨平台统一的交互矩形命令。
//
// 把「macOS NSTimer 动态切换 ignoresMouseEvents」与「Windows 后端轮询 +
// set_ignore_cursor_events 动态穿透」收敛成前端只调一个 `update_interactive_rects`。
// 命令本身定义在独立模块(非 crate 根),避免与 generate_handler! 在同模块产生的
// __cmd__ 宏重定义冲突。
//
// 内部按 #[cfg] 分派到平台模块的 store_* 实现;非 mac/win 平台(Linux)目前无局部
// 穿透,直接 no-op。

use crate::geometry::Rect;

#[tauri::command]
pub fn update_interactive_rects(rects: Vec<Rect>) {
    #[cfg(target_os = "macos")]
    crate::macos_pet::store_interactive_rects(&rects);
    #[cfg(target_os = "windows")]
    crate::windows_pet::store_hit_rects(&rects);
    // 其他平台(Linux 等)目前无局部穿透,no-op。
}
