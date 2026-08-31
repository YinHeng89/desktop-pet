// 宠物帧/动作相关纯类型（被 frame.ts / actionScheduler.ts 复用，零 Vue 依赖）。

export interface FrameSeq {
  row: number
  count: number
  fps: number
}

export interface PetFrame {
  width: number
  height: number
  cols: number
  rows: number
}

export interface PetFrameSource {
  idle?: FrameSeq
  talk?: FrameSeq
  actions?: Record<string, FrameSeq>
  frame?: PetFrame
}
