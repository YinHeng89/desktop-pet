// Vitest 全局初始化（每个测试文件运行前执行）
//
// 目的：jsdom 只实现了 DOM，未实现 Canvas / Image 等浏览器 API。
// 本项目 SpritePet 通过 canvas 2d context 绘制精灵帧，若不提供 mock，
// 组件测试会在 getContext('2d') 处直接抛错。
//
// 这里只做「最小可用」的注入，真实绘制行为由被测代码决定；
// 调用记录写入 helpers.ts 的 canvasDrawCalls，供用例断言。
// 如需更细粒度的控制（如断言 imageSmoothingEnabled），请在用例内用 vi.spyOn 覆盖。

import { vi } from 'vitest'
import { createContext2DMock } from './helpers'

HTMLCanvasElement.prototype.getContext = vi.fn((type: string) =>
  type === '2d' ? createContext2DMock() : null,
) as unknown as HTMLCanvasElement['getContext']
