// Tauri 事件监听 composable
//
// 解决的问题：PetHost 曾在 onMounted 里直接调用 onEvent(...) 注册 9 个监听，
// 却一个都没保存返回值，onUnmounted 也没清理——组件销毁后监听继续存活。
// 由于 listen() 是异步的，手写清理还要处理「组件先卸载、listen 后 resolve」的竞态，
// 每个调用点都写一遍既啰嗦又容易漏，故统一收敛到本 composable。
//
// 使用约束：**必须在 <script setup> 顶层（或 onMounted 的同步段）调用**。
// 与 watch 同理，一旦越过 await，Vue 的 currentInstance 已被重置，
// onUnmounted 就无法与组件实例关联（详见 PetHost.vue 中 notify watch 的修复注释）。

import { onMounted, onUnmounted } from 'vue'
import { onEvent } from '../tauri'

type Unlisten = () => void

/**
 * 注册一个 Tauri 事件监听，并在组件卸载时自动取消。
 *
 * - 非 Tauri 环境（浏览器 dev）为 no-op，与 onEvent 行为一致。
 * - 处理异步竞态：若组件在 listen() resolve 之前就已卸载，
 *   会在 resolve 后立刻调用取消函数，不会留下悬挂监听。
 */
export function useTauriEvent(event: string, handler: (payload: unknown) => void): void {
  let unlisten: Unlisten | null = null
  // 卸载前置标记：既阻止「已卸载但仍回调」，也处理「卸载先于 listen resolve」。
  let disposed = false

  onMounted(() => {
    void onEvent(event, (payload) => {
      if (!disposed) handler(payload)
    }).then((un) => {
      if (disposed) {
        // 组件已在 listen 完成前卸载，立刻取消，避免悬挂监听
        un()
        return
      }
      unlisten = un
    })
  })

  onUnmounted(() => {
    disposed = true
    unlisten?.()
    unlisten = null
  })
}
