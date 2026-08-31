// 测试辅助工具（PetBuddy）
//
// 与 setup.ts 的分工：
//   - setup.ts  : 只负责「全局副作用」（注入浏览器 API mock），不导出任何东西
//   - helpers.ts: 只负责「可被用例导入的能力」与跨用例共享的状态
//
// 二者由 vitest 在同一模块注册表内加载，因此 canvasDrawCalls 数组在
// setup 注入的 mock 与用例断言之间共享同一实例。

import { vi } from 'vitest'

export interface CanvasDrawCall {
  type: 'clearRect' | 'drawImage'
  args: unknown[]
}

/**
 * 累计的 canvas 绘制调用序列（由 setup.ts 注入的 getContext mock 写入）。
 *
 * 存在的意义：SpritePet 的两条回归断言依赖它——
 *   - P1-3「缩放闪空白帧」→ 断言 scale 变化后**立即**出现 drawImage
 *   - P1-4「切宠残影」    → 断言换宠物时先出现 clearRect
 * 详见 docs/refactor/TEST_PLAN.md §5.1。
 */
export const canvasDrawCalls: CanvasDrawCall[] = []

/** 清空绘制调用记录（建议在每个用例的 beforeEach 中调用） */
export function resetCanvasDrawCalls(): void {
  canvasDrawCalls.length = 0
}

/**
 * 构造最小的 canvas 2d 上下文 mock。
 * 只实现本项目实际用到的 API；其余方法缺失时会直接在用例中暴露，便于按需补充。
 */
export function createContext2DMock(): CanvasRenderingContext2D {
  const record =
    (type: CanvasDrawCall['type']) =>
    (...args: unknown[]) => {
      canvasDrawCalls.push({ type, args })
    }

  return {
    clearRect: record('clearRect'),
    drawImage: record('drawImage'),
  } as unknown as CanvasRenderingContext2D
}

/** 让出事件循环，等待所有已排队的微任务与 0ms 定时器执行完毕 */
export function flushPromises(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0)
  })
}

/** 构造一个已 resolve 的 Image，用于精灵图加载路径的测试 */
export function mockImageLoad(width = 1536, height = 2288): void {
  vi.stubGlobal(
    'Image',
    class {
      crossOrigin = ''
      naturalWidth = width
      naturalHeight = height
      onload: (() => void) | null = null
      onerror: (() => void) | null = null
      private _src = ''
      get src(): string {
        return this._src
      }
      set src(v: string) {
        this._src = v
        // 与浏览器一致：src 赋值后异步触发 onload
        setTimeout(() => this.onload?.(), 0)
      }
    },
  )
}
