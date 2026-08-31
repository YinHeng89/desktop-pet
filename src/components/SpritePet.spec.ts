// SpritePet 组件测试
//
// 重点覆盖两条已实际发生过的 bug 的回归：
//   - P1-3「缩放闪空白帧」：给 canvas.width 赋值会清空画布，必须立即重绘
//   - P1-4「切宠残影」    ：新图加载完成前，画布上仍留着上一只宠物的最后一帧
// 详见 docs/refactor/TEST_PLAN.md §5.1。
//
// 注意：每个用例都必须 unmount。SpritePet 在 onMounted 里启动了
// requestAnimationFrame 循环，若不卸载会跨用例持续运行，
// 既泄漏资源又会污染 canvasDrawCalls 记录。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import SpritePet from './SpritePet.vue'
import { petStore } from '../store/pet'
import { canvasDrawCalls, mockImageLoad, resetCanvasDrawCalls } from '../test/helpers'
import { makePet, resetPetStore, setupPetStore } from '../test/fixtures/pets'

describe('SpritePet', () => {
  let wrapper: VueWrapper | null = null

  beforeEach(() => {
    resetPetStore()
    setupPetStore()
    resetCanvasDrawCalls()
    mockImageLoad(1536, 2288)
  })

  afterEach(() => {
    wrapper?.unmount()
    wrapper = null
    vi.unstubAllGlobals()
  })

  /** 挂载并等 img.onload 派发完毕（mock 中用 setTimeout(0) 触发），使 imgLoaded 置 true */
  async function mountSprite(scale = 1): Promise<VueWrapper> {
    wrapper = mount(SpritePet, { props: { state: 'idle', scale } })
    await nextTick()
    await new Promise((resolve) => setTimeout(resolve, 0))
    return wrapper
  }

  it('按 帧宽 × scale 设置 canvas 尺寸与 CSS 尺寸', async () => {
    const w = await mountSprite()
    const canvas = w.find('canvas').element as HTMLCanvasElement

    expect(canvas.width).toBe(192)
    expect(canvas.height).toBe(208)
    expect(canvas.style.width).toBe('192px')
  })

  it('缩放后立即重绘当前帧（回归 P1-3：缩放闪空白帧）', async () => {
    const w = await mountSprite()
    resetCanvasDrawCalls()

    await w.setProps({ scale: 1.3 })

    // 关键回归点：canvas.width 被重新赋值后画布已被清空，
    // 若不在同一个 watch 里补一次 drawImage，画面会空到下一个动画间隔才恢复。
    expect(canvasDrawCalls.some((c) => c.type === 'drawImage')).toBe(true)

    const canvas = w.find('canvas').element as HTMLCanvasElement
    expect(canvas.width).toBe(Math.round(192 * 1.3))
  })

  it('切换宠物时先清空画布（回归 P1-4：切宠残影）', async () => {
    await mountSprite()
    resetCanvasDrawCalls()

    // 切到另一只宠物（新图的 onload 尚未触发）
    petStore.pets = [makePet({ id: 'another', displayName: '另一只', dir: 'another' })]
    petStore.currentId = 'another'
    await nextTick()

    // 关键回归点：切宠后第一个绘制动作必须是 clearRect，
    // 否则旧宠物的最后一帧会残留到新图加载完成为止。
    expect(canvasDrawCalls[0]?.type).toBe('clearRect')
  })

  it('卸载时取消 requestAnimationFrame', async () => {
    const cancelSpy = vi.spyOn(window, 'cancelAnimationFrame')
    await mountSprite()

    wrapper?.unmount()
    wrapper = null

    expect(cancelSpy).toHaveBeenCalled()
    cancelSpy.mockRestore()
  })
})
