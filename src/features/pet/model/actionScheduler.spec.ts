import { describe, it, expect } from 'vitest'
import {
  RANDOM_POOL,
  actionDurationMs,
  pickRandomAction,
  nextRandomDelayMs,
} from './actionScheduler'
import type { FrameSeq } from './types'

const actions: Record<string, FrameSeq> = {
  wave: { row: 1, count: 6, fps: 10 },
  jump: { row: 2, count: 4, fps: 8 },
}

describe('actionDurationMs', () => {
  it('count/fps*1000', () => {
    expect(actionDurationMs({ row: 0, count: 8, fps: 8 })).toBe(1000)
    expect(actionDurationMs({ row: 0, count: 6, fps: 10 })).toBe(600)
  })
  it('fps 缺失回退 8', () => {
    expect(actionDurationMs({ row: 0, count: 8, fps: 0 })).toBe(1000)
  })
})

describe('pickRandomAction', () => {
  it('talk 状态不播随机动作', () => {
    expect(pickRandomAction(RANDOM_POOL, actions, 'talk')).toBeNull()
  })
  it('无可用动作返回 null', () => {
    expect(pickRandomAction(RANDOM_POOL, {}, 'idle')).toBeNull()
  })
  it('在可用动作中等概率选取（rng 可控）', () => {
    expect(pickRandomAction(RANDOM_POOL, actions, 'idle', () => 0)).toBe('wave')
    expect(pickRandomAction(RANDOM_POOL, actions, 'idle', () => 0.999)).toBe('jump')
  })
})

describe('nextRandomDelayMs', () => {
  it('落在 [6000,15000)', () => {
    expect(nextRandomDelayMs(() => 0)).toBe(6000)
    const v = nextRandomDelayMs(() => 0.999)
    expect(v).toBeGreaterThanOrEqual(6000)
    expect(v).toBeLessThan(15000)
  })
})
