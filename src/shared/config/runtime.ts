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
