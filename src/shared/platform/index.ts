// 平台能力获取（消灭 UA 嗅探）。
//
// `initPlatform()` 在 App.vue onMounted 时调一次 Rust `get_platform` 命令填充；
// 之后 `getPlatform()` 同步返回，业务代码据此选策略（如拖拽用原生还是 DOM 方案）。
// 未初始化时按环境给出安全 fallback，避免首屏空窗。

import { isTauri } from '../config/runtime'
import { invokeTyped } from '../ipc/client'
import type { Platform } from './types'

let platform: Platform | null = null

/** 在应用启动时调用一次：从 Rust 拉取真实平台并缓存。 */
export async function initPlatform(): Promise<void> {
  if (!isTauri) {
    platform = 'web'
    return
  }
  try {
    const p = await invokeTyped<Platform>('get_platform')
    platform = p
  } catch {
    platform = 'unknown'
  }
}

/** 同步返回已初始化的平台；未初始化时按环境降级（web / unknown）。 */
export function getPlatform(): Platform {
  if (platform) return platform
  return isTauri ? 'unknown' : 'web'
}

/** 仅供测试重置。 */
export function resetPlatform(): void {
  platform = null
}
