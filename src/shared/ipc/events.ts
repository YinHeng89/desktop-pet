// 跨窗口事件总线（替代组件里手写的 emit/listen，统一 source 防回环）。
//
// - `emitCrossWindow`：自动给 payload 包一层 `{ source, payload }`。
// - `onCrossWindow`：跳过 `source === 我的窗口` 的事件，杜绝跨窗口回声（R5）。
//
// 底层仍走 Rust `broadcast_event`（invoke → app.emit 广播到所有窗口），
// 因此跨窗口可靠；source 字段仅用于前端侧去重。

import { emitEvent, onEvent } from '../../tauri'
import { windowLabel } from '../config/runtime'

export interface CrossWindowPayload<T = unknown> {
  source: string
  payload: T
}

function wrap<T>(payload: T): CrossWindowPayload<T> {
  return { source: windowLabel(), payload }
}

/** 跨窗口广播（自动附加 source 标记）。 */
export async function emitCrossWindow<T>(event: string, payload: T): Promise<void> {
  await emitEvent(event, wrap(payload))
}

/**
 * 监听跨窗口事件；自动跳过自己发出的回声。
 * 返回取消监听函数。非 Tauri 环境返回空函数。
 */
export async function onCrossWindow(
  event: string,
  handler: (payload: unknown) => void,
): Promise<() => void> {
  const myWindow = windowLabel()
  return onEvent(event, (raw: unknown) => {
    const wrapped = raw as Partial<CrossWindowPayload> | null
    if (
      wrapped &&
      typeof wrapped === 'object' &&
      'source' in wrapped &&
      wrapped.source === myWindow
    ) {
      return
    }
    handler(raw)
  })
}
