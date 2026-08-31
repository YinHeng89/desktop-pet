// useTauriEvent 回归测试
//
// 覆盖 P1-1：PetHost 曾在 onMounted 里注册 9 个 Tauri 事件监听，
// 一个都没保存返回值、onUnmounted 也没清理——组件销毁后监听继续存活。
// 而 listen() 是异步的，手写清理还要额外处理「组件先卸载、listen 后 resolve」
// 的竞态，每个调用点都写一遍既啰嗦又容易漏，故收敛为本 composable。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount } from '@vue/test-utils'
import { useTauriEvent } from './useTauriEvent'

// 记录每个 useTauriEvent 注册时拿到的取消函数，便于断言「是否都被调用」。
// vi.mock 会被 Vitest 提升到 import 之前，因此下面静态 import 的
// useTauriEvent 拿到的正是这里的 mock 版 onEvent。
const unlisteners: Array<ReturnType<typeof vi.fn>> = []

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  return {
    ...actual,
    onEvent: vi.fn((_event: string) => {
      const un = vi.fn()
      unlisteners.push(un)
      return Promise.resolve(un)
    }),
  }
})

/** 用渲染函数构造测试组件（避免依赖运行时模板编译） */
function makeComp(eventCount: number) {
  return defineComponent({
    setup() {
      for (let i = 0; i < eventCount; i++) {
        useTauriEvent(`test-event-${i}`, () => {})
      }
      return () => h('div')
    },
  })
}

describe('useTauriEvent', () => {
  beforeEach(() => {
    unlisteners.length = 0
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('挂载时注册监听', async () => {
    mount(makeComp(3))
    // onEvent 是异步的，等 listen resolve
    await Promise.resolve()

    expect(unlisteners).toHaveLength(3)
    expect(unlisteners.every((u) => u && typeof u === 'function')).toBe(true)
  })

  it('卸载时取消全部监听（回归 P1-1：监听曾永久泄漏）', async () => {
    const wrapper = mount(makeComp(3))
    await Promise.resolve()
    expect(unlisteners).toHaveLength(3)

    wrapper.unmount()

    for (const un of unlisteners) {
      expect(un).toHaveBeenCalledTimes(1)
    }
  })

  it('组件在 listen resolve 之前就卸载时，resolve 后立即取消（不留下悬挂监听）', async () => {
    // 验证异步竞态分支：disposed 置位后，then 回调里会立刻调用 un()
    const wrapper = mount(makeComp(1))

    // 不等 listen 的 promise resolve 就卸载
    wrapper.unmount()
    await Promise.resolve()
    await Promise.resolve()

    expect(unlisteners).toHaveLength(1)
    expect(unlisteners[0]).toHaveBeenCalledTimes(1)
  })

  it('重复挂载/卸载多个实例互不干扰', async () => {
    const w1 = mount(makeComp(2))
    const w2 = mount(makeComp(2))
    await Promise.resolve()
    expect(unlisteners).toHaveLength(4)

    w1.unmount()
    // 只有 w1 的两个被取消
    expect(unlisteners[0]).toHaveBeenCalledTimes(1)
    expect(unlisteners[1]).toHaveBeenCalledTimes(1)
    expect(unlisteners[2]).not.toHaveBeenCalled()
    expect(unlisteners[3]).not.toHaveBeenCalled()

    w2.unmount()
    expect(unlisteners[2]).toHaveBeenCalledTimes(1)
    expect(unlisteners[3]).toHaveBeenCalledTimes(1)
  })
})
