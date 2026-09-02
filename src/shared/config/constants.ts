// 所有跨语言共享的魔数唯一真源（★ 单一真源）。
//
// 这些值同时被 Rust（`domain/`）与前端使用。现阶段前端从这里取值，Rust 侧保留
// 字面量；Phase 9.5 会引入 ts-rs 由 Rust 生成同值文件，CI 校验两份一致（R6 常量单一真源）。
// 新增任何跨端常量都必须加到这里，禁止在组件里再写一份。

/** 宠物帧逻辑尺寸（scale=1）。与 domain::pet 的 FRAME_W/H 同源。 */
export const FRAME = {
  WIDTH: 192,
  HEIGHT: 208,
  COLS: 8,
} as const

/** 缩放范围与步进（与前端滑块、Rust clamp_scale 对齐）。 */
export const SCALE = {
  MIN: 0.5,
  MAX: 1.3,
  STEP: 0.05,
  DEFAULT: 0.7,
} as const

/** 窗口几何：基线宽、四周缓冲、屏幕边缘留白（避开 Dock/任务栏）。 */
export const WINDOW = {
  BASE_WIDTH: 320,
  PAD: 24,
  EDGE_GAP_X: 24,
  EDGE_GAP_Y_MAC: 75,
  EDGE_GAP_Y_WIN: 64,
} as const

/** 本地通知 HTTP 服务与外部调用约束。 */
export const NOTIFY = {
  PORT: 8756,
  MAX_LEN: 120,
  DEFAULT_DURATION_MS: 4000,
} as const

/** 气泡视觉与动画参数。 */
export const BUBBLE = {
  MAX_WIDTH: 300,
  SHADOW_PAD: 28,
  PET_SHADOW_PAD: 16,
  ENTER_MS: 300,
  LEAVE_MS: 220,
  SETTLE_MS: 200,
} as const

/** 拖拽阈值与时序（区分点击与拖拽）。 */
export const DRAG = {
  /** 宠物窗口：位移超过该值才判定为拖拽（否则视为手抖/单击）。 */
  THRESHOLD_PX: 6,
  /** 设置窗口标题栏拖动阈值（略小于宠物窗口，标题栏拖动要求更灵敏）。 */
  SETTINGS_THRESHOLD_PX: 5,
  /** 窗口停止移动多久判定拖拽结束（Windows mouseup 常被 OS 吞掉，靠它兜底）。 */
  MOVED_DEBOUNCE_MS: 180,
  /** 拖拽松手后忽略紧跟的 click 的时长，避免松手被误判成单击。 */
  CLICK_GUARD_MS: 80,
} as const

/** 闲时动作与气泡停留时序。 */
export const TIMING = {
  RANDOM_MIN_MS: 6000,
  RANDOM_MAX_MS: 15000,
  CHAT_MS: 3000,
} as const
