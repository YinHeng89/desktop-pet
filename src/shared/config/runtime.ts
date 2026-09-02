// 运行时环境探测（与平台无关，零 Vue 依赖）。
//
// 单一真源：业务代码统一从这里取 `isTauri` / 当前窗口 label，
// 不再各自嗅探 `window.__TAURI_INTERNALS__`。后续 Phase 7 会把 tauri.ts 里的
// 同名定义收敛到此处，消除重复。

/** 是否运行在 Tauri 壳内（而非纯浏览器）。 */
export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

/**
 * 当前 webview 所属窗口的 label（多窗口路由用：main=宠物窗口，settings=设置窗口）。
 * 同步读 Tauri 注入的 `__TAURI_INTERNALS__.metadata.currentWindow.label`，
 * 该字段在 webview 创建时即存在，首帧即可正确路由，无竞态。
 */
export function windowLabel(): string {
  if (typeof window === 'undefined') return 'main'
  try {
    const internals = (
      window as unknown as {
        __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } }
      }
    ).__TAURI_INTERNALS__
    return internals?.metadata?.currentWindow?.label ?? 'main'
  } catch {
    return 'main'
  }
}

/**
 * 从 UA 粗判平台 —— **全项目唯一允许嗅探 navigator 的地方**。
 *
 * 业务代码禁止直接使用，统一走 `shared/platform` 的 `getPlatform()` / `isMacOS()`：
 * 那里优先采用 Rust `get_platform` 命令的权威结果，UA 只在命令尚未返回
 * （首帧 / 纯浏览器 dev 预览）时作为降级依据。
 */
export function detectPlatformFromUA(): 'macos' | 'windows' | 'linux' | 'unknown' {
  if (typeof navigator === 'undefined') return 'unknown'
  const p = navigator.platform ?? ''
  const ua = navigator.userAgent ?? ''
  if (/Mac|iPhone|iPad|iPod/i.test(p) || /Macintosh/i.test(ua)) return 'macos'
  if (/Win/i.test(p) || /Windows/i.test(ua)) return 'windows'
  if (/Linux|X11/i.test(p) || /Linux/i.test(ua)) return 'linux'
  return 'unknown'
}
