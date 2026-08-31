// 类型化 IPC 客户端（业务代码统一走这里，禁止在组件里直接 invoke）。
//
// - `invokeTyped<T>`：统一 catch → 抛 `AppError`（含 code），便于 UI 映射文案（R7）。
// - 非 Tauri 环境直接抛 `Platform` 错误，调用方据此降级。
// - 集中日志（带命令名），便于排查。
//
// 现有 tauri.ts 的逐项封装仍保留作兼容；Phase 7 组件拆分时会逐步迁移到本客户端。

import { isTauri } from '../config/runtime'
import { AppError, type ErrorCode } from '../errors/AppError'

export interface InvokeOpts {
  /** 预留：未来可注入超时（当前 Tauri invoke 无原生超时，靠命令侧保障）。 */
  timeoutMs?: number
}

type CoreInvoke = (cmd: string, args: Record<string, unknown>) => Promise<unknown>

let coreInvoke: CoreInvoke | null = null

async function loadInvoke(): Promise<CoreInvoke> {
  if (!isTauri) {
    throw new AppError('Platform', '当前非 Tauri 环境，无法调用命令')
  }
  if (!coreInvoke) {
    const core = await import('@tauri-apps/api/core')
    coreInvoke = core.invoke as CoreInvoke
  }
  return coreInvoke
}

/** 类型化调用 Rust 命令。失败时统一包装为 AppError。 */
export async function invokeTyped<T>(
  cmd: string,
  args?: Record<string, unknown>,
  _opts?: InvokeOpts,
): Promise<T> {
  const invoke = await loadInvoke()
  try {
    const result = await invoke(cmd, args ?? {})
    return result as T
  } catch (e) {
    throw toAppError(cmd, e)
  }
}

function toAppError(cmd: string, e: unknown): AppError {
  const message =
    typeof e === 'string' ? e : ((e as { message?: string } | undefined)?.message ?? '未知错误')
  const code: ErrorCode = classify(cmd, message)
  return new AppError(code, `[${cmd}] ${message}`, e)
}

/** 依据命令名与原始信息粗分错误类别（供 UI 选文案）。 */
function classify(cmd: string, message: string): ErrorCode {
  if (/pet|zip|import/i.test(cmd) && /id|invalid|illegal/i.test(message)) {
    return 'InvalidPetId'
  }
  if (/pet|zip/i.test(cmd) && /path|escape|\.\./i.test(message)) return 'ZipSlip'
  if (/too large|large|8 ?kb|size/i.test(message)) return 'TooLarge'
  if (/network|reqwest|timeout|connect/i.test(message)) return 'Network'
  if (/json|parse|serialize|deserialize/i.test(message)) return 'Serialization'
  if (/io|read|write|file/i.test(message)) return 'Io'
  return 'Unknown'
}
