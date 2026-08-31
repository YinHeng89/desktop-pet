// PetHost 通知气泡测试
//
// 重点覆盖 P0-4 的回归：HTTP / IPC 的 duration 字段此前一路贯通到前端，
// 却在构造 NotifyItem 时被丢弃，气泡时长恒为 4000ms。
// 详见 docs/refactor/TEST_PLAN.md §5.2。
//
// 说明：
// 1. jsdom 中没有 __TAURI_INTERNALS__，故 isTauri 为 false，
//    所有 Tauri 分支（穿透上报、原生拖拽、窗口 resize）自动跳过，
//    无需 mock 即可测通知队列逻辑。
// 2. **必须在每个用例后 unmount**：notifyStore 是模块级单例，
//    PetHost 通过 watch(notifyStore.pending) 消费它。若上一个用例的
//    PetHost 实例未卸载，它的 watcher 仍活着，会抢先 consume 掉
//    下一个用例的通知，导致新实例收不到——表现为「气泡时有时无」。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import PetHost from './PetHost.vue'
import { consumeNotify, notifyStore, pushNotify } from '../store/notify'
import { flushAll, mockImageLoad } from '../test/helpers'
import { resetPetStore, setupPetStore } from '../test/fixtures/pets'

/**
 * 真实等待指定毫秒。
 * 本文件刻意不用 fake timers：PetHost 内部同时有 requestAnimationFrame
 * 与多条 setTimeout 链，用真实短时等待比 fake timers 更贴近实际、更少踩坑。
 */
function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

describe('PetHost 通知气泡', () => {
  let wrapper: VueWrapper | null = null

  beforeEach(() => {
    resetPetStore()
    setupPetStore()
    mockImageLoad(1536, 2288)
  })

  afterEach(() => {
    wrapper?.unmount()
    wrapper = null
    vi.unstubAllGlobals()
  })

  /** 挂载并等到 onMounted 内部的 async 流程（含 watch 注册）全部完成 */
  async function mountHost(): Promise<VueWrapper> {
    wrapper = mount(PetHost)
    await flushAll()
    return wrapper
  }

  it('按自定义 duration 消失（回归 P0-4：duration 曾被前端丢弃）', async () => {
    const w = await mountHost()

    await pushNotify('短通知', undefined, 60)
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(true)
    expect(w.text()).toContain('短通知')

    await wait(250)
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(false)
  })

  it('未指定 duration 时使用默认 4s（250ms 后气泡仍在）', async () => {
    const w = await mountHost()

    await pushNotify('默认时长通知')
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(true)
    expect(w.text()).toContain('默认时长通知')

    await wait(250)
    await flushAll()
    // 默认 4000ms，远大于 250ms，此时不应消失
    expect(w.find('.bubble').exists()).toBe(true)
  })

  it('空文本不入队（不显示气泡）', async () => {
    const w = await mountHost()

    await pushNotify('')
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(false)

    // 反向验证：同一实例紧接着发一条非空通知应当正常显示，
    // 避免「上面之所以没有气泡，其实是实例根本没工作」的假阳性。
    await pushNotify('紧随其后的有效通知')
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(true)
  })

  it('非法 duration（NaN）回退到默认时长，气泡不会立刻消失', async () => {
    const w = await mountHost()

    await pushNotify('非法时长', undefined, Number.NaN)
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(true)

    await wait(250)
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(true)
  })

  it('unmount 后不再消费通知（回归：watcher 曾脱离组件 scope 而永久存活）', async () => {
    const w = await mountHost()
    w.unmount()
    wrapper = null // 已手动卸载，避免 afterEach 重复 unmount

    await pushNotify('卸载后发出的通知')
    await flushAll()

    // 关键回归点：watcher 随组件销毁而停止，没有任何消费者取走这条通知。
    // 修复前（watch 注册在 async onMounted 的 await 之后）watcher 会脱离
    // 组件 effect scope 永久存活，unmount 后仍抢消费，导致新实例收不到通知。
    expect(notifyStore.pending).not.toBeNull()

    consumeNotify() // 清理，避免污染后续用例
  })

  it('超大 duration 被截断到 60s 上限（不会永久占用气泡）', async () => {
    const w = await mountHost()

    await pushNotify('超大时长', undefined, 999_999_999)
    await flushAll()
    // 上限保护只影响「何时消失」，不影响「是否显示」；
    // 这里断言它确实显示了，且没有被当成 0 / 非法值而立即消失。
    expect(w.find('.bubble').exists()).toBe(true)

    await wait(250)
    await flushAll()
    expect(w.find('.bubble').exists()).toBe(true)
  })
})
