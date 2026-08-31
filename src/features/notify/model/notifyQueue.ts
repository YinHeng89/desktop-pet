// 通知气泡队列纯逻辑（从 PetHost.vue 抽出，★ 零 DOM/定时器依赖，可单测）。
//
// 抽纯动机：归一化时长、FIFO 入队、动作优先于气泡等纯规则曾混在 setTimeout 回调里，
// 改动易回归。状态与定时器由 Phase 7 的 useNotifyQueue 持有；此处只管数据与纯规则。

export const DEFAULT_NOTIFY_MS = 4000
export const MAX_NOTIFY_MS = 60_000

/** 归一化气泡时长：非法值（非数字/≤0）回退默认，超出上限截断。 */
export function normalizeDuration(d: unknown): number {
  const n = Number(d)
  if (!Number.isFinite(n) || n <= 0) return DEFAULT_NOTIFY_MS
  return Math.min(n, MAX_NOTIFY_MS)
}

export interface NotifyItem {
  id: number
  text: string
  action?: string
  /** 气泡显示时长(ms)。 */
  duration?: number
}

/** 是否应「先播动作再显示气泡」（动作名存在且非 talk）。 */
export function shouldPlayActionFirst(item: NotifyItem): boolean {
  return !!item.action && item.action !== 'talk'
}

/** 纯 FIFO 通知队列（与渲染状态解耦）。 */
export class NotifyQueue {
  private items: NotifyItem[] = []
  private seq = 0

  enqueue(payload: { text?: string; action?: string; duration?: number }): NotifyItem | null {
    const text = payload?.text ?? ''
    if (!text) return null
    const item: NotifyItem = {
      id: ++this.seq,
      text,
      action: payload?.action,
      duration: payload?.duration,
    }
    this.items.push(item)
    return item
  }

  dequeue(): NotifyItem | null {
    return this.items.shift() ?? null
  }

  peek(): NotifyItem | null {
    return this.items[0] ?? null
  }

  get size(): number {
    return this.items.length
  }
}
