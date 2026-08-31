// 鼠标穿透命中矩形纯计算（从 PetHost.vue reportInteractiveRects 抽出，★ 零 DOM 依赖）。
//
// 抽纯动机：气泡/宠物矩形需按 scale 外扩阴影区（防 Windows SetWindowRgn 裁掉阴影），
// 且「宠物未就绪却只有气泡矩形」时必须上报空数组保持整窗可交互。这套边界逻辑曾
// 与 DOM 测量、定时器混在一起，回归风险高；抽出后可单测。

export type Rect4 = [number, number, number, number] // [left, top, width, height]

/** 气泡阴影外扩基准（×scale）；.bubble 阴影最大约 24px，保守放宽到 28。 */
export const BUBBLE_SHADOW_PAD = 28
/** 宠物区 drop-shadow 外扩基准（×scale）；最大约 12px，保守放宽到 16。 */
export const PET_SHADOW_PAD = 16

export interface MeasuredRect {
  left: number
  top: number
  width: number
  height: number
}

/** 矩形四周外扩 pad（覆盖阴影/模糊溢出）。 */
export function padRect(rect: MeasuredRect, pad: number): Rect4 {
  return [rect.left - pad, rect.top - pad, rect.width + pad * 2, rect.height + pad * 2]
}

export interface HitRectInput {
  /** 已测量的气泡矩形（调用方需先过滤 width/height>0）。 */
  bubble?: MeasuredRect | null
  /** 已测量的宠物矩形。 */
  pet?: MeasuredRect | null
  scale: number
}

export interface HitRectResult {
  rects: Rect4[]
  hasPetRect: boolean
}

/**
 * 计算需要上报的可交互矩形列表。
 * 关键守卫：宠物未就绪（pet 为 null）时，若只有气泡矩形，返回空数组 ——
 * 避免 Windows 用残缺矩形把整个宠物区裁掉导致点不动（启动竞态偶发异常）。
 */
export function computeHitRects(input: HitRectInput): HitRectResult {
  const bubblePad = BUBBLE_SHADOW_PAD * input.scale
  const petPad = PET_SHADOW_PAD * input.scale
  const rects: Rect4[] = []
  if (input.bubble) rects.push(padRect(input.bubble, bubblePad))
  const hasPetRect = !!input.pet
  if (input.pet) rects.push(padRect(input.pet, petPad))
  if (!hasPetRect && rects.length > 0) return { rects: [], hasPetRect: false }
  return { rects, hasPetRect }
}
