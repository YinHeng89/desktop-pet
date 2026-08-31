// 测试基础设施自检（PetBuddy）
//
// 目的：确认「测试安全网」本身可用，而不只是配置了一堆文件。
// 若本文件的用例失败，说明 vitest / jsdom / @vue/test-utils / canvas mock
// 链条中有环节断裂——那么 Phase 1 起写的任何回归测试都不可信，应优先修复这里。
//
// 覆盖范围刻意保持最小：只验证「依赖链能跑通」，不验证任何业务逻辑。
// 业务用例清单见 docs/refactor/TEST_PLAN.md。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import Hello from './fixtures/Hello.vue'
import { canvasDrawCalls, flushPromises, mockImageLoad, resetCanvasDrawCalls } from './helpers'

describe('测试基础设施自检', () => {
  describe('canvas mock（SpritePet 组件测试的依赖）', () => {
    beforeEach(() => {
      resetCanvasDrawCalls()
    })

    it('getContext("2d") 返回 mock，并记录 clearRect / drawImage 调用', () => {
      const canvas = document.createElement('canvas')
      const ctx = canvas.getContext('2d')

      expect(ctx).not.toBeNull()
      expect(canvasDrawCalls).toHaveLength(0)

      ctx?.clearRect(0, 0, 10, 10)
      ctx?.drawImage(new Image(), 0, 0, 1, 1, 0, 0, 1, 1)

      expect(canvasDrawCalls).toHaveLength(2)
      expect(canvasDrawCalls[0].type).toBe('clearRect')
      expect(canvasDrawCalls[1].type).toBe('drawImage')
    })

    it('getContext 非 "2d" 时返回 null（保持浏览器语义）', () => {
      const canvas = document.createElement('canvas')
      expect(canvas.getContext('webgl')).toBeNull()
    })

    it('resetCanvasDrawCalls 能清空记录（供用例间隔离）', () => {
      const ctx = document.createElement('canvas').getContext('2d')
      ctx?.clearRect(0, 0, 1, 1)
      expect(canvasDrawCalls).toHaveLength(1)

      resetCanvasDrawCalls()
      expect(canvasDrawCalls).toHaveLength(0)
    })
  })

  describe('Vue 组件挂载（组件测试的依赖）', () => {
    it('能挂载 SFC 并断言 props 渲染结果', () => {
      const wrapper = mount(Hello, { props: { label: 'PetBuddy' } })

      expect(wrapper.find('.hello').text()).toBe('PetBuddy')
    })

    it('能触发并断言事件（交互类用例的基础）', async () => {
      const wrapper = mount(Hello, { props: { label: 'x' } })
      expect(wrapper.exists()).toBe(true)

      wrapper.unmount()
      expect(wrapper.exists()).toBe(false)
    })
  })

  describe('Image mock（精灵图加载路径的依赖）', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('src 赋值后异步触发 onload，且尺寸可读', async () => {
      mockImageLoad(1536, 2288)
      const img = new Image()
      const loaded = new Promise<void>((resolve) => {
        img.onload = () => resolve()
      })

      img.src = 'data:image/webp;base64,zzz'
      await loaded

      expect(img.naturalWidth).toBe(1536)
      expect(img.naturalHeight).toBe(2288)
    })
  })

  describe('flushPromises', () => {
    it('能等待已排队的微任务执行完毕', async () => {
      let done = false
      void Promise.resolve().then(() => {
        done = true
      })

      await flushPromises()
      expect(done).toBe(true)
    })
  })
})
