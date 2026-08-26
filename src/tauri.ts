// PetBuddy 的 Tauri 封装（精简版）：仅宠物所需能力。
// 双窗口架构（main=宠物窗口，settings=设置窗口），
// 跨窗口状态通过 emitEvent（前端→前端全局广播）与 Rust command 同步。

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

// 当前 webview 所属窗口的 label（用于多窗口路由：main=宠物窗口，settings=设置窗口）。
// 同步读 Tauri 注入的 __TAURI_INTERNALS__.metadata.currentWindow.label。
// 该字段在 webview 创建时即存在（不依赖任何异步注入），首帧即可正确路由，无竞态。
export function currentWindowLabel(): string {
  if (typeof window === 'undefined') return 'main'
  try {
    const internals = (window as unknown as {
      __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } }
    }).__TAURI_INTERNALS__
    return internals?.metadata?.currentWindow?.label ?? 'main'
  } catch {
    return 'main'
  }
}

// Tauri API 模块在运行时按需加载，但 startDragging 需在 mousedown 内同步调用，
// 故预加载 window API；同时预加载 core(invoke)，避免首次上报可交互矩形时
// 动态 import 造成的额外延迟（该延迟会赶不上 macOS 50ms 穿透轮询，导致鼠标误穿透）。
let windowApi: typeof import('@tauri-apps/api/window') | null = null
let coreApi: typeof import('@tauri-apps/api/core') | null = null

export async function preloadTauri(): Promise<void> {
  if (!isTauri) return
  if (!windowApi) {
    windowApi = await import('@tauri-apps/api/window')
  }
  if (!coreApi) {
    coreApi = await import('@tauri-apps/api/core')
  }
}

/** 同步拿到 invoke（若已预加载则直接用，避免动态 import 延迟） */
async function getInvoke(): Promise<typeof import('@tauri-apps/api/core')['invoke']> {
  if (coreApi) return coreApi.invoke
  const core = await import('@tauri-apps/api/core')
  coreApi = core
  return core.invoke
}

/** 同步调用系统级 startDragging（必须在 mousedown 内同步调用） */
export function startDragging(): Promise<void> {
  if (!isTauri || !windowApi) return Promise.resolve()
  try {
    return windowApi.getCurrentWindow().startDragging()
  } catch (e) {
    console.error('[tauri] startDragging failed', e)
    return Promise.resolve()
  }
}

/** 注册 Tauri 事件监听（Rust → 前端） */
export async function onEvent(
  event: string,
  handler: (payload: unknown) => void,
): Promise<() => void> {
  if (!isTauri) return () => {}
  const { listen } = await import('@tauri-apps/api/event')
  const un = await listen(event, (e) => handler(e.payload))
  return () => un()
}

/** 跨窗口广播事件（前端 → 前端，如设置窗口改缩放/显隐/切换宠物同步给 main 窗口）。
 *  走 Rust command broadcast_event（invoke → app.emit），规避前端 emit 跨窗口不生效的问题。 */
export async function emitEvent(event: string, payload: unknown = undefined): Promise<void> {
  if (!isTauri) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('broadcast_event', { event, payload: payload ?? null })
  } catch (e) {
    console.error(`[tauri] emitEvent('${event}') failed`, e)
  }
}

/** 上报可交互矩形（macOS 像素穿透用） */
export async function setNotifyInteractiveRects(
  rects: Array<[number, number, number, number]>,
): Promise<void> {
  if (!isTauri) return
  try {
    const invoke = await getInvoke()
    await invoke('set_notify_interactive_rects', { rects })
  } catch (e) {
    console.error('[tauri] setNotifyInteractiveRects failed', e)
  }
}

/** 上报可交互矩形（Windows 透明区域穿透用，SetWindowRgn 裁切） */
export async function setPetHitRects(
  rects: Array<[number, number, number, number]>,
): Promise<void> {
  if (!isTauri) return
  try {
    const invoke = await getInvoke()
    await invoke('set_pet_hit_rects', { rects })
  } catch (e) {
    console.error('[tauri] setPetHitRects failed', e)
  }
}

/** 触发 Windows 端把当前 hit rects 应用到窗口（SetWindowRgn 即时生效） */
export async function applyPetHitRects(): Promise<void> {
  if (!isTauri) return
  try {
    const invoke = await getInvoke()
    await invoke('apply_pet_hit_rects')
  } catch (e) {
    console.error('[tauri] applyPetHitRects failed', e)
  }
}

/** 显示/隐藏宠物窗口（visible 开关联动整个窗口） */
export async function showPetWindow(): Promise<void> {
  if (!isTauri) return
  try {
    const w = await windowApi!.Window.getByLabel('main')
    await w?.show()
  } catch (e) {
    console.error('[tauri] showPetWindow failed', e)
  }
}
export async function hidePetWindow(): Promise<void> {
  if (!isTauri) return
  try {
    // 走 Rust command 而非 windowApi.hide()：Rust 内部会先 SetWindowRgn(0) 清空
    // 像素级裁切区域，再 hide，避免 Windows DWM 按旧 region 渲染装饰闪现窗口边框。
    const invoke = await getInvoke()
    await invoke('hide_pet_window')
  } catch (e) {
    console.error('[tauri] hidePetWindow failed', e)
  }
}

/** 按缩放比例重设 main 宠物窗口尺寸（窗口跟随宠物缩放） */
export async function resizePetWindow(scale: number): Promise<void> {
  if (!isTauri) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('resize_pet_window', { scale })
  } catch (e) {
    console.error('[tauri] resizePetWindow failed', e)
  }
}

/** 打开设置窗口（Rust command 负责 show/center/focus） */
export async function openSettingsWindow(): Promise<void> {
  if (!isTauri) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_settings_window')
  } catch (e) {
    console.error('[tauri] openSettingsWindow failed', e)
  }
}

/** 用系统默认浏览器打开外部链接 */
export async function openExternal(url: string): Promise<void> {
  if (!isTauri) {
    window.open(url, '_blank')
    return
  }
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_external', { url })
  } catch (e) {
    console.error('[tauri] openExternal failed', e)
    window.open(url, '_blank')
  }
}

/** 关闭（隐藏）设置窗口 */
export async function closeSettingsWindow(): Promise<void> {
  if (!isTauri) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('close_settings_window')
  } catch (e) {
    console.error('[tauri] closeSettingsWindow failed', e)
  }
}

/** 设置开机自启 */
export async function setAutoStart(enabled: boolean): Promise<void> {
  if (!isTauri) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('set_autostart', { enabled })
  } catch (e) {
    console.error('[tauri] setAutoStart failed', e)
  }
}

/** 查询开机自启状态 */
export async function getAutoStart(): Promise<boolean> {
  if (!isTauri) return false
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    return (await invoke<boolean>('get_autostart')) ?? false
  } catch (e) {
    console.error('[tauri] getAutoStart failed', e)
    return false
  }
}

// ── 在线画廊（awesome-codex-pet）──

export interface OnlinePetMeta {
  slug: string
  name: string
  author: string
  category: string
  description: string
  sprite_version: number
  preview_url: string
}

/** 浏览在线宠物列表（拉取 awesome-codex-pet 索引） */
export async function browseOnlinePets(): Promise<OnlinePetMeta[]> {
  if (!isTauri) return []
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    return (await invoke<OnlinePetMeta[]>('browse_online_pets')) ?? []
  } catch (e) {
    console.error('[tauri] browseOnlinePets failed', e)
    return []
  }
}

/** 下载指定 slug 的在线宠物（返回宠物定义，自动注册进宠物列表） */
export async function downloadOnlinePet(slug: string): Promise<unknown> {
  if (!isTauri) return null
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    return await invoke('download_online_pet', { slug })
  } catch (e) {
    console.error('[tauri] downloadOnlinePet failed', e)
    throw e
  }
}
