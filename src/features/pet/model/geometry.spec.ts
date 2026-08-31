import { describe, it, expect } from 'vitest'
import { padRect, computeHitRects, BUBBLE_SHADOW_PAD, PET_SHADOW_PAD } from './geometry'

describe('padRect', () => {
  it('四周外扩', () => {
    expect(padRect({ left: 10, top: 20, width: 100, height: 40 }, 5)).toEqual([5, 15, 110, 50])
  })
})

describe('computeHitRects', () => {
  it('宠物就绪：气泡+宠物两矩形，均外扩阴影', () => {
    const { rects, hasPetRect } = computeHitRects({
      bubble: { left: 0, top: 0, width: 50, height: 30 },
      pet: { left: 0, top: 0, width: 100, height: 200 },
      scale: 1,
    })
    expect(hasPetRect).toBe(true)
    expect(rects).toHaveLength(2)
    expect(rects[0]).toEqual([
      -BUBBLE_SHADOW_PAD,
      -BUBBLE_SHADOW_PAD,
      50 + BUBBLE_SHADOW_PAD * 2,
      30 + BUBBLE_SHADOW_PAD * 2,
    ])
    expect(rects[1]).toEqual([
      -PET_SHADOW_PAD,
      -PET_SHADOW_PAD,
      100 + PET_SHADOW_PAD * 2,
      200 + PET_SHADOW_PAD * 2,
    ])
  })
  it('仅气泡（宠物未就绪）→ 空数组，避免裁掉宠物区', () => {
    const { rects, hasPetRect } = computeHitRects({
      bubble: { left: 0, top: 0, width: 50, height: 30 },
      pet: null,
      scale: 1,
    })
    expect(hasPetRect).toBe(false)
    expect(rects).toEqual([])
  })
  it('仅宠物 → 单矩形，scale 影响外扩', () => {
    const { rects, hasPetRect } = computeHitRects({
      pet: { left: 0, top: 0, width: 100, height: 200 },
      scale: 2,
    })
    expect(hasPetRect).toBe(true)
    expect(rects).toHaveLength(1)
    expect(rects[0][2]).toBe(100 + PET_SHADOW_PAD * 2 * 2)
  })
})
