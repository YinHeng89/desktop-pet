// 平台能力获取（消灭 UA 嗅探）。
//
// `initPlatform()` 在 App.vue onMounted 时调一次 Rust `get_platform` 命令填充；
// 之后 `getPlatform()` 同步返回，业务代码据此选策略（如拖拽用原生还是 DOM 方案）。
// 未初始化时按环境给出安全 fallback，避免首屏空窗。

import { isTauri, detectPlatformFromUA } from '../config/runtime'
import { invokeTyped } from '../ipc/client'
import type { Platform } from './types'

let platform: Platform | null = null

/**
 * 在应用启动时调用一次：从 Rust 拉取真实平台并缓存。
 * 失败不抛（平台只是策略选择的输入，不该阻断启动），退化为 unknown。
 */
export async function initPlatform(): Promise<void> {
  if (!isTauri) {
    platform = 'web'
    return
  }
  try {
    platform = await invokeTyped<Platform>('get_platform')
  } catch {
    platform = 'unknown'
  }
}

/** 同步返回已初始化的平台；未初始化时按环境降级（web / unknown）。 */
export function getPlatform(): Platform {
  if (platform) return platform
  return isTauri ? 'unknown' : 'web'
}

/**
 * 是否 macOS —— 统一入口，业务代码不得再自己嗅探 navigator。
 *
 * 优先级：Rust 探测结果 > UA 嗅探（降级）。
 * 降级很重要：`get_platform` 是异步 invoke，首帧与纯浏览器 dev 预览时
 * 结果尚未返回，若此时直接判 false，macOS 上会错误地改用 DOM 拖拽/悬停方案
 * （已知在 App 非激活时不可靠，是 macos_pet.rs 存在的根本原因）。
 */
export function isMacOS(): boolean {
  const p = getPlatform()
  if (p === 'macos') return true
  if (p === 'windows' || p === 'linux') return false
  return detectPlatformFromUA() === 'macos'
}

/** 仅供测试重置。 */
export function resetPlatform(): void {
  platform = null
}
