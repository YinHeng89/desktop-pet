// 命令名常量（与 Rust `generate_handler!` 一一对应）。
//
// 业务代码用 `COMMANDS.xxx` 而非裸字符串，避免拼写漂移；
// Phase 9.5 会由 Rust 生成同值清单，CI 校验一致（R6）。

export const COMMANDS = {
  // 系统
  quitApp: 'quit_app',
  openExternal: 'open_external',
  getPlatform: 'get_platform',
  broadcastEvent: 'broadcast_event',
  // 自启
  setAutostart: 'set_autostart',
  getAutostart: 'get_autostart',
  wasAutoStarted: 'was_auto_started',
  // 窗口
  resizePetWindow: 'resize_pet_window',
  openSettingsWindow: 'open_settings_window',
  closeSettingsWindow: 'close_settings_window',
  hidePetWindow: 'hide_pet_window',
  showPetWindow: 'show_pet_window',
  // 穿透
  updateInteractiveRects: 'update_interactive_rects',
  setNotifyInteractiveRects: 'set_notify_interactive_rects',
  setPetHitRects: 'set_pet_hit_rects',
  applyPetHitRects: 'apply_pet_hit_rects',
  // 宠物导入/管理
  importPet: 'import_pet',
  listImportedPets: 'list_imported_pets',
  deleteImportedPet: 'delete_imported_pet',
  updateImportedPet: 'update_imported_pet',
  browseOnlinePets: 'browse_online_pets',
  downloadOnlinePet: 'download_online_pet',
  // 通知
  pushNotify: 'push_notify',
} as const

export type CommandName = (typeof COMMANDS)[keyof typeof COMMANDS]
