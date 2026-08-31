import { describe, it, expect } from 'vitest'
import { pickDialogue, BUILTIN_DIALOGUES, EXTERNAL_DIALOGUES } from './dialogues'

describe('pickDialogue', () => {
  it('内置宠物按 id 取性格台词', () => {
    const lines = BUILTIN_DIALOGUES.miku.wave
    const r = pickDialogue('miku', 'wave', () => 0)
    expect(lines).toContain(r)
  })
  it('未知 id 回退外部通用台词', () => {
    const lines = EXTERNAL_DIALOGUES.wave
    expect(lines).toContain(pickDialogue('unknown-pet', 'wave', () => 0.5))
  })
  it('无对应台词返回空串', () => {
    expect(pickDialogue('miku', 'nonexistent', () => 0)).toBe('')
  })
})
