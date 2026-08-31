// 在线宠物画廊筛选纯逻辑（从 PetSettings.vue 抽出，★ 零 Vue 依赖，可单测）。

export interface GalleryFilterable {
  name: string
  author: string
  category: string
}

/** 按关键词（不区分大小写）在 name/author/category 中匹配；空关键词返回原列表。 */
export function filterOnlinePets<T extends GalleryFilterable>(list: T[], keyword: string): T[] {
  const kw = keyword.trim().toLowerCase()
  if (!kw) return list
  return list.filter(
    (p) =>
      p.name.toLowerCase().includes(kw) ||
      p.author.toLowerCase().includes(kw) ||
      p.category.toLowerCase().includes(kw),
  )
}
