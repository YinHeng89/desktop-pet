import { describe, it, expect } from 'vitest'
import { arrayBufferToBase64 } from './base64'

describe('arrayBufferToBase64', () => {
  it('Hello → SGVsbG8=', () => {
    const buf = new TextEncoder().encode('Hello').buffer
    expect(arrayBufferToBase64(buf)).toBe('SGVsbG8=')
  })
})
