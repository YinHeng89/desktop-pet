import { describe, it, expect } from 'vitest'
import { seqFor, frameBounds, isFrameInBounds } from './frame'
import type { PetFrameSource } from './types'

const pet: PetFrameSource = {
  idle: { row: 0, count: 8, fps: 8 },
  talk: { row: 1, count: 8, fps: 8 },
  actions: { wave: { row: 2, count: 6, fps: 10 } },
  frame: { width: 192, height: 208, cols: 8, rows: 11 },
}

describe('seqFor', () => {
  it('返回 idle/talk 段', () => {
    expect(seqFor('idle', pet)).toEqual({ row: 0, count: 8, fps: 8 })
    expect(seqFor('talk', pet)).toEqual({ row: 1, count: 8, fps: 8 })
  })
  it('返回 action 段', () => {
    expect(seqFor('wave', pet)).toEqual({ row: 2, count: 6, fps: 10 })
  })
  it('未知动作回退 idle；无 idle 时 null', () => {
    expect(seqFor('jump', pet)).toEqual(pet.idle)
    expect(seqFor('wave', null)).toBeNull()
  })
})

describe('frameBounds', () => {
  it('标准模板 192x208×8列11行', () => {
    expect(frameBounds(192 * 8, 208 * 11, { width: 192, height: 208, cols: 8, rows: 11 })).toEqual({
      cols: 8,
      rows: 11,
    })
  })
  it('非标高清包按真实图尺寸算列/行', () => {
    expect(frameBounds(256 * 6, 256 * 6, { width: 256, height: 256, cols: 8, rows: 11 })).toEqual({
      cols: 6,
      rows: 6,
    })
  })
})

describe('isFrameInBounds', () => {
  const b = { rows: 11, cols: 8 }
  it('边界内为 true', () => {
    expect(isFrameInBounds(10, 7, b)).toBe(true)
  })
  it('越界 row/col 为 false', () => {
    expect(isFrameInBounds(11, 0, b)).toBe(false)
    expect(isFrameInBounds(0, 8, b)).toBe(false)
  })
})
