// 测试用 Tauri IPC mock（使前端逻辑可在 jsdom 下纯单测，无需真实 Tauri 壳）。
//
// 用法（在 *.spec.ts 顶部）：
//   import { mockInvoke, setInvokeHandler, invocationLog } from '../mocks/tauri'
//   vi.mock('@tauri-apps/api/core', () => ({ invoke: mockInvoke }))
//
// 随后用 `setInvokeHandler((cmd, args) => ...)` 设定桩，`invocationLog()` 断言调用。

type InvokeHandler = (cmd: string, args: Record<string, unknown>) => unknown

const listeners = new Map<string, Set<(payload: unknown) => void>>()
let invocations: { cmd: string; args: Record<string, unknown> }[] = []
let handler: InvokeHandler = () => {
  throw new Error('mockInvoke handler 未设置：请先调用 setInvokeHandler')
}

/** 设定 invoke 桩实现（返回值与拒绝均可）。 */
export function setInvokeHandler(fn: InvokeHandler): void {
  handler = fn
}

/** 重置所有 mock 状态（每个 spec 的 beforeEach 调用）。 */
export function resetTauriMock(): void {
  invocations = []
  listeners.clear()
  handler = () => {
    throw new Error('mockInvoke handler 未设置：请先调用 setInvokeHandler')
  }
}

/** 返回至今所有 invoke 调用记录，供断言。 */
export function invocationLog(): { cmd: string; args: Record<string, unknown> }[] {
  return invocations
}

/** 模拟 `@tauri-apps/api/core` 的 invoke（供 vi.mock 映射）。 */
export async function mockInvoke(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  invocations.push({ cmd, args })
  return handler(cmd, args)
}

/** 模拟 Rust 侧 `app.emit(event, payload)`：通知已注册的前端监听。 */
export function emitEvent(name: string, payload: unknown): void {
  listeners.get(name)?.forEach((fn) => fn(payload))
}

/** 注册监听（供模拟 `@tauri-apps/api/event` 的 listen）。返回取消函数。 */
export function addEventListener(name: string, fn: (payload: unknown) => void): () => void {
  const set = listeners.get(name) ?? new Set<(payload: unknown) => void>()
  set.add(fn)
  listeners.set(name, set)
  return () => {
    set.delete(fn)
  }
}
