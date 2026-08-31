import { describe, it, expect } from 'vitest'
import { filterOnlinePets } from './filter'

const pets = [
  { name: 'Miku', author: 'crypton', category: 'singer' },
  { name: 'Ryu', author: 'bandai', category: 'hero' },
]

describe('filterOnlinePets', () => {
  it('空关键词返回全部', () => {
    expect(filterOnlinePets(pets, '')).toHaveLength(2)
    expect(filterOnlinePets(pets, '   ')).toHaveLength(2)
  })
  it('按 name/author/category 匹配（大小写不敏感）', () => {
    expect(filterOnlinePets(pets, 'mik')).toEqual([pets[0]])
    expect(filterOnlinePets(pets, 'BANDAI')).toEqual([pets[1]])
    expect(filterOnlinePets(pets, 'SINGER')).toEqual([pets[0]])
    expect(filterOnlinePets(pets, 'zzz')).toEqual([])
  })
})
