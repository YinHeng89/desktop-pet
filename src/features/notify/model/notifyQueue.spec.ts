import { describe, it, expect } from 'vitest'
import {
  normalizeDuration,
  shouldPlayActionFirst,
  NotifyQueue,
  DEFAULT_NOTIFY_MS,
  MAX_NOTIFY_MS,
} from './notifyQueue'

describe('normalizeDuration', () => {
  it('非法/非正 → 默认', () => {
    expect(normalizeDuration('')).toBe(DEFAULT_NOTIFY_MS)
    expect(normalizeDuration(0)).toBe(DEFAULT_NOTIFY_MS)
    expect(normalizeDuration(-5)).toBe(DEFAULT_NOTIFY_MS)
    expect(normalizeDuration('abc')).toBe(DEFAULT_NOTIFY_MS)
  })
  it('正常回传并截断上限', () => {
    expect(normalizeDuration(3000)).toBe(3000)
    expect(normalizeDuration(999999)).toBe(MAX_NOTIFY_MS)
  })
})

describe('shouldPlayActionFirst', () => {
  it('wave 先播动作', () => {
    expect(shouldPlayActionFirst({ id: 1, text: 'x', action: 'wave' })).toBe(true)
  })
  it('talk 或不带 action 不优先', () => {
    expect(shouldPlayActionFirst({ id: 1, text: 'x', action: 'talk' })).toBe(false)
    expect(shouldPlayActionFirst({ id: 1, text: 'x' })).toBe(false)
  })
})

describe('NotifyQueue', () => {
  it('空 text 不入队', () => {
    const q = new NotifyQueue()
    expect(q.enqueue({ text: '' })).toBeNull()
    expect(q.size).toBe(0)
  })
  it('FIFO 出队', () => {
    const q = new NotifyQueue()
    q.enqueue({ text: 'a' })
    q.enqueue({ text: 'b', action: 'wave' })
    expect(q.size).toBe(2)
    expect(q.dequeue()?.text).toBe('a')
    expect(q.dequeue()?.text).toBe('b')
    expect(q.dequeue()).toBeNull()
  })
})
