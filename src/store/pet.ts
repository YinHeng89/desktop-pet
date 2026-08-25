// 桌面宠物状态管理（PetBuddy 独立应用）。
// 双窗口架构：main=宠物窗口，settings=设置窗口，各自持有独立 store 实例，
// 跨窗口状态通过 emitEvent（前端→前端广播）同步。
import { reactive, computed } from 'vue'
import petManifest from '../pets/manifest.json'

export interface FrameSeq {
  row: number
  count: number
  fps: number
}
export interface PetDef {
  id: string
  displayName: string
  description: string
  dir: string
  spritesheet: string
  idle: FrameSeq
  talk: FrameSeq
  actions?: Record<string, FrameSeq>
  external?: boolean
}
export interface PetManifest {
  frame: { width: number; height: number; cols: number }
  pets: PetDef[]
}

interface PetStore {
  pets: PetDef[]
  frame: { width: number; height: number; cols: number }
  currentId: string
  scale: number
  visible: boolean
}

const STORAGE_KEY_ID = 'petbuddy_id'
const STORAGE_KEY_SCALE = 'petbuddy_scale'
const STORAGE_KEY_VISIBLE = 'petbuddy_visible'

export const MIN_SCALE = 0.8
export const MAX_SCALE = 1.3

function loadId(): string {
  try {
    return localStorage.getItem(STORAGE_KEY_ID) || ''
  } catch {
    return ''
  }
}
function loadScale(): number {
  try {
    const v = Number(localStorage.getItem(STORAGE_KEY_SCALE))
    if (v >= MIN_SCALE && v <= MAX_SCALE) return Math.round(v * 20) / 20
    return 1
  } catch {
    return 1
  }
}
function loadVisible(): boolean {
  try {
    const v = localStorage.getItem(STORAGE_KEY_VISIBLE)
    return v === null ? true : v !== 'false'
  } catch {
    return true
  }
}

export const petStore = reactive<PetStore>({
  pets: [],
  frame: { width: 192, height: 208, cols: 8 },
  currentId: loadId(),
  scale: loadScale(),
  visible: loadVisible(),
})

export const currentPet = computed<PetDef | null>(() => {
  if (!petStore.pets.length) return null
  return petStore.pets.find((p) => p.id === petStore.currentId) || petStore.pets[0]
})

/** 加载内置宠物清单 + 外部导入宠物 */
export async function loadPetManifest(): Promise<void> {
  try {
    const data = petManifest as unknown as PetManifest
    petStore.pets = data.pets || []
    if (data.frame) petStore.frame = data.frame
  } catch (e) {
    console.error('[petStore] 加载宠物 manifest 失败:', e)
  }
  await loadExternalPets()
  if (!petStore.pets.some((p) => p.id === petStore.currentId)) {
    petStore.currentId = petStore.pets[0]?.id || ''
  }
}

async function loadExternalPets(): Promise<void> {
  try {
    const core = await import('@tauri-apps/api/core')
    const list = await core.invoke<Array<{
      id: string
      display_name: string
      description: string
      spritesheet: string
      idle: FrameSeq
      talk: FrameSeq
      actions: Record<string, FrameSeq>
    }>>('list_imported_pets')
    if (!list || !Array.isArray(list)) return
    for (const ext of list) {
      if (petStore.pets.some((p) => p.id === ext.id)) continue
      petStore.pets.push({
        id: ext.id,
        displayName: ext.display_name,
        description: ext.description,
        dir: ext.id,
        spritesheet: ext.spritesheet,
        idle: ext.idle,
        talk: ext.talk,
        actions: ext.actions,
        external: true,
      })
    }
  } catch (e) {
    console.error('[petStore] 加载外部宠物失败:', e)
  }
}

export async function importExternalPet(base64: string, fileName: string): Promise<PetDef> {
  const core = await import('@tauri-apps/api/core')
  const ext = await core.invoke<{
    id: string
    display_name: string
    description: string
    spritesheet: string
    idle: FrameSeq
    talk: FrameSeq
    actions: Record<string, FrameSeq>
  }>('import_pet', { base64, fileName })
  if (!ext) throw new Error('导入失败：Rust 返回空')

  const pet: PetDef = {
    id: ext.id,
    displayName: ext.display_name,
    description: ext.description,
    dir: ext.id,
    spritesheet: ext.spritesheet,
    idle: ext.idle,
    talk: ext.talk,
    actions: ext.actions,
    external: true,
  }
  const idx = petStore.pets.findIndex((p) => p.id === ext.id)
  if (idx >= 0) petStore.pets[idx] = pet
  else petStore.pets.push(pet)
  return pet
}

/** 注册在线下载的宠物（download_online_pet 返回结构与 import_pet 一致） */
export function registerDownloadedPet(ext: {
  id: string
  display_name: string
  description: string
  spritesheet: string
  idle: FrameSeq
  talk: FrameSeq
  actions: Record<string, FrameSeq>
}): PetDef {
  const pet: PetDef = {
    id: ext.id,
    displayName: ext.display_name,
    description: ext.description,
    dir: ext.id,
    spritesheet: ext.spritesheet,
    idle: ext.idle,
    talk: ext.talk,
    actions: ext.actions,
    external: true,
  }
  const idx = petStore.pets.findIndex((p) => p.id === ext.id)
  if (idx >= 0) petStore.pets[idx] = pet
  else petStore.pets.push(pet)
  return pet
}

export async function deleteExternalPet(id: string): Promise<void> {
  const core = await import('@tauri-apps/api/core')
  await core.invoke('delete_imported_pet', { id })
  const idx = petStore.pets.findIndex((p) => p.id === id)
  if (idx >= 0) petStore.pets.splice(idx, 1)
  if (petStore.currentId === id) {
    // 删除的是当前选中的宠物 → 回退到第一个宠物，并广播切换事件让 main 窗口同步
    petStore.currentId = petStore.pets[0]?.id || ''
    try {
      localStorage.setItem(STORAGE_KEY_ID, petStore.currentId)
    } catch {
      /* ignore */
    }
    // 关键：通知 main 窗口（桌面宠物）同步切换到新宠物，否则桌面宠物仍停留在已删除的宠物
    import('../tauri').then((m) => m.emitEvent('pet-switch', petStore.currentId)).catch(() => {})
  }
}

export function setCurrentPet(id: string): void {
  const pet = petStore.pets.find((p) => p.id === id)
  if (!pet) return
  petStore.currentId = id
  try {
    localStorage.setItem(STORAGE_KEY_ID, id)
  } catch {
    /* ignore */
  }
  // 跨窗口广播：main 窗口的 PetHost 监听 pet-switch 后同步切换（settings 窗口修改时实时生效）
  import('../tauri').then((m) => m.emitEvent('pet-switch', id)).catch(() => {})
}

export function setPetScale(scale: number): void {
  const clamped = Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale))
  const s = Math.round(clamped * 20) / 20
  petStore.scale = s
  try {
    localStorage.setItem(STORAGE_KEY_SCALE, String(s))
  } catch {
    /* ignore */
  }
  // 跨窗口广播：main 窗口的 PetHost 监听 pet-scale 后实时同步缩放
  import('../tauri').then((m) => m.emitEvent('pet-scale', s)).catch(() => {})
}

export function setPetVisible(v: boolean): void {
  petStore.visible = v
  try {
    localStorage.setItem(STORAGE_KEY_VISIBLE, String(v))
  } catch {
    /* ignore */
  }
  // 跨窗口广播：main 窗口的 PetHost 监听 pet-visible 后实时显示/隐藏
  import('../tauri').then((m) => m.emitEvent('pet-visible', v)).catch(() => {})
}

export function openPetPicker(): void {
  // 设置界面现为独立 settings 窗口，双击宠物/托盘「打开设置」时打开该窗口。
  // 浏览器 dev（无 Tauri）下无法打开独立窗口，静默跳过（设置功能仅桌面端可用）。
  import('../tauri').then((m) => m.openSettingsWindow()).catch(() => {})
}
