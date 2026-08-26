// 本地通知 store：PetSettings（设置窗口：测试通知/导入提示）与 PetHost（main 窗口：气泡显示）共享。
// 多窗口架构下，settings 与 main 是独立 webview，各自有独立的 store 内存。
// 故 pushNotify 在 Tauri 环境通过全局事件 'notify-push' 广播给 main 窗口（PetHost 已监听该事件），
// 与 Rust HTTP 服务 notify_server 的广播共用同一事件名，PetHost 无需区分来源。
import { reactive } from 'vue'
import { isTauri } from '../tauri'

export interface NotifyPayload {
  id: number
  text: string
  action?: string
  duration?: number
}

interface NotifyStore {
  /** 最新一条待消费的通知（同一 webview 内的 PetHost watch 消费后置空） */
  pending: NotifyPayload | null
}

let seq = 0

export const notifyStore = reactive<NotifyStore>({
  pending: null,
})

/** 推一条本地通知（测试通知/导入提示用）。
 *  Tauri 下通过 invoke 调用 Rust command `push_notify`，由 Rust 侧 app.emit 广播给 main 窗口的 PetHost。
 *  走 Tauri IPC，绕过 HTTP/CORS 与前端 emit 跨窗口的时序问题，与外部 HTTP 通知共用同一事件名。
 *  返回 Promise：后端字数硬限制等错误会 reject，调用方 await/catch 可拿到提示。
 *  浏览器 dev 下写入本窗口 store（PetHost watch pending 同窗口消费）。 */
export function pushNotify(text: string, action?: string, duration?: number): Promise<void> {
  if (isTauri) {
    return import('@tauri-apps/api/core')
      .then(({ invoke }) => invoke('push_notify', { text, action, duration }))
      .catch((e) => {
        console.error('[notify] push_notify 调用失败:', e)
        throw e
      })
  }

  // 浏览器 dev：同窗口消费（PetHost watch pending）
  notifyStore.pending = { id: ++seq, text, action, duration }
  return Promise.resolve()
}

/** 消费待处理通知 */
export function consumeNotify(): NotifyPayload | null {
  const p = notifyStore.pending
  notifyStore.pending = null
  return p
}
