// 精灵帧布局纯计算（从 SpritePet.vue 抽出，★ 零 DOM 依赖，可单测）。
//
// 抽纯动机：外部宠物精灵图行/列数不统一（如 miku 9 行、标准模板 11 行），
// 切帧越界会「宠物闪没」。越界保护逻辑必须可单测，避免改尺寸算法时回归。

import type { FrameSeq, PetFrame, PetFrameSource } from './types'

/** 按动作状态取对应帧段：talk/idle/actions[state]，未知动作回退 idle。 */
export function seqFor(state: string, pet: PetFrameSource | null | undefined): FrameSeq | null {
  if (!pet) return null
  if (state === 'talk') return pet.talk ?? null
  if (state === 'idle') return pet.idle ?? null
  const a = pet.actions?.[state]
  return a ?? pet.idle ?? null
}

/** 用运行时真实图尺寸算实际行列数（外部包尺寸各异）。 */
export function frameBounds(
  imgW: number,
  imgH: number,
  frame: PetFrame,
): { rows: number; cols: number } {
  const cols = Math.floor(imgW / frame.width)
  const rows = Math.floor(imgH / frame.height)
  return { rows, cols }
}

/** 帧是否越界（row/col 在真实行列范围内）。 */
export function isFrameInBounds(
  row: number,
  col: number,
  bounds: { rows: number; cols: number },
): boolean {
  return row >= 0 && col >= 0 && row < bounds.rows && col < bounds.cols
}
