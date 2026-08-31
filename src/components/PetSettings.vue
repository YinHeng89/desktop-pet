<script setup lang="ts">
// 宠物设置页（settings 窗口整页内容）。
import { onMounted, onUnmounted, ref, computed } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import {
  petStore,
  currentPet,
  setCurrentPet,
  setPetScale,
  setPetVisible,
  importExternalPet,
  deleteExternalPet,
  updateExternalPet,
  registerDownloadedPet,
  loadPetManifest,
  MIN_SCALE,
  MAX_SCALE,
  type PetDef,
} from '../store/pet'
import { pushNotify } from '../store/notify'
import {
  closeSettingsWindow,
  startDragging,
  browseOnlinePets,
  downloadOnlinePet,
  openExternal,
  preloadTauri,
  onEvent,
  type OnlinePetMeta,
} from '../tauri'
import SpritePet from './SpritePet.vue'

// settings 窗口是独立 webview，pets 由 App.vue onMounted 异步加载；
// 此处兜底：挂载时若尚未加载则主动加载，保证列表与预览始终有数据。
// 当前选中的宠物由共享 localStorage（loadId 在 store 初始化时读取）决定，
// currentPet 已改为「未匹配时返回 null」而非回退第一个，故不会首屏闪现第一个宠物。
onMounted(() => {
  // settings 是独立 webview 窗口，必须在此预加载 windowApi，
  // 否则 startDragging 因 windowApi 为 null 而直接 return，窗口无法拖动。
  void preloadTauri()
  if (!petStore.pets.length) {
    void loadPetManifest()
  }
  // 禁用右键菜单（避免无边框窗口里弹出 webview 默认菜单）
  document.addEventListener('contextmenu', (e) => e.preventDefault())

  // 同步：托盘 / main 窗口切换「显示宠物」后，本设置窗口的开关也实时更新（修复只发不收的失同步）
  onEvent('pet-visible', (payload) => {
    petStore.visible = payload !== 'false' && payload !== false
  }).then((u) => {
    unlistenVisible = u
  })

  // 同步：托盘 / main 窗口切换宠物后，本设置窗口的选中项与预览实时更新。
  // 注意：这里只被动改 currentId 高亮，不能调 setCurrentPet（那会再广播形成回环）；
  // 持久化已由触发方（setCurrentPet）完成，此处仅同步 UI。
  onEvent('pet-switch', (payload) => {
    const id = String(payload).replace(/^pet:/, '')
    if (petStore.pets.some((p) => p.id === id)) {
      petStore.currentId = id
    }
  }).then((u) => {
    unlistenSwitch = u
  })

  // 左侧底部版本号：读取 Tauri 打包时嵌入的版本（来自 tauri.conf.json）
  getVersion()
    .then((v) => {
      appVersion.value = v
    })
    .catch(() => {})
})

let unlistenVisible: (() => void) | null = null
let unlistenSwitch: (() => void) | null = null
onUnmounted(() => {
  if (unlistenVisible) {
    unlistenVisible()
    unlistenVisible = null
  }
  if (unlistenSwitch) {
    unlistenSwitch()
    unlistenSwitch = null
  }
})

// 设置界面左下角显示的版本号（来自 tauri.conf.json）
const appVersion = ref('')

function onSelect(id: string): void {
  setCurrentPet(id)
}

// 删除外部宠物（二次点击确认，避免 window.confirm 在无边框窗口不可用）
const pendingDeleteId = ref<string | null>(null)
let pendingDeleteTimer: ReturnType<typeof setTimeout> | null = null

function onDeleteClick(p: PetDef): void {
  if (pendingDeleteId.value !== p.id) {
    // 第一次点击：进入确认态，3 秒后自动恢复
    pendingDeleteId.value = p.id
    if (pendingDeleteTimer) clearTimeout(pendingDeleteTimer)
    pendingDeleteTimer = setTimeout(() => {
      pendingDeleteId.value = null
    }, 3000)
    return
  }
  // 第二次点击：确认删除
  void doDelete(p)
}

async function doDelete(p: PetDef): Promise<void> {
  if (pendingDeleteTimer) {
    clearTimeout(pendingDeleteTimer)
    pendingDeleteTimer = null
  }
  pendingDeleteId.value = null
  try {
    await deleteExternalPet(p.id)
    pushNotify(`宠物「${p.displayName}」已删除`)
  } catch (e) {
    console.error('[PetSettings] 删除宠物失败:', e)
    pushNotify('删除失败，请重试')
  }
}

// ── 编辑外部宠物元信息（名字 + 描述）──
const editPetOpen = ref(false)
const editPetId = ref('')
const editPetName = ref('')
const editPetDesc = ref('')
const editPetSaving = ref(false)
const editPetError = ref('')
function openEditPet(p: PetDef): void {
  editPetId.value = p.id
  editPetName.value = p.displayName
  editPetDesc.value = p.description
  editPetError.value = ''
  editPetSaving.value = false
  editPetOpen.value = true
}
function closeEditPet(): void {
  editPetOpen.value = false
}
async function saveEditPet(): Promise<void> {
  if (editPetSaving.value) return
  editPetSaving.value = true
  editPetError.value = ''
  try {
    await updateExternalPet(editPetId.value, {
      displayName: editPetName.value,
      description: editPetDesc.value,
    })
    pushNotify(`宠物信息已更新`, 'wave')
    closeEditPet()
  } catch (e) {
    console.error('[PetSettings] 编辑宠物失败:', e)
    editPetError.value = (e as Error)?.message || String(e)
  } finally {
    editPetSaving.value = false
  }
}

// ── 缩放滑块（自绘：div 轨道 + 填充 + 圆点，像素级对齐）──
// 复用 store 的 MIN_SCALE/MAX_SCALE（下限已放宽到 0.5），避免两处定义不一致
const trackEl = ref<HTMLElement | null>(null)

function scaleToPercent(scale: number): number {
  return ((scale - MIN_SCALE) / (MAX_SCALE - MIN_SCALE)) * 100
}
function positionToScale(clientX: number): number {
  const track = trackEl.value
  if (!track) return 1
  const rect = track.getBoundingClientRect()
  const ratio = (clientX - rect.left) / rect.width
  const clamped = Math.min(1, Math.max(0, ratio))
  const raw = MIN_SCALE + clamped * (MAX_SCALE - MIN_SCALE)
  return Math.round(raw * 20) / 20
}
let dragging = false
function onTrackPointerDown(e: PointerEvent): void {
  dragging = true
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  setPetScale(positionToScale(e.clientX))
}
function onTrackPointerMove(e: PointerEvent): void {
  if (!dragging) return
  setPetScale(positionToScale(e.clientX))
}
function onTrackPointerUp(): void {
  dragging = false
}
function onToggleVisible(e: Event): void {
  const checked = (e.target as HTMLInputElement).checked
  setPetVisible(checked)
}

// ── 测试通知弹窗（自定义内容）──
const notifyModalOpen = ref(false)
const notifyText = ref('')
const notifyError = ref('')
// 发送冷却：点击发送后进入 3 秒冷却，按钮显示「冷却中 Ns」并禁用，倒计时结束恢复
const COOLDOWN_SECS = 3
const cooldownLeft = ref(0)
let cooldownTimer: ReturnType<typeof setInterval> | null = null
function openNotifyModal(): void {
  notifyText.value = ''
  notifyError.value = ''
  cooldownLeft.value = 0
  if (cooldownTimer) {
    clearInterval(cooldownTimer)
    cooldownTimer = null
  }
  notifyModalOpen.value = true
}
function closeNotifyModal(): void {
  notifyModalOpen.value = false
}
async function sendNotify(): Promise<void> {
  const text = notifyText.value.trim()
  if (!text || cooldownLeft.value > 0) return
  try {
    await pushNotify(text)
    notifyError.value = ''
    // 不关闭弹窗，进入冷却倒计时
    cooldownLeft.value = COOLDOWN_SECS
    if (cooldownTimer) clearInterval(cooldownTimer)
    cooldownTimer = setInterval(() => {
      cooldownLeft.value -= 1
      if (cooldownLeft.value <= 0) {
        cooldownLeft.value = 0
        if (cooldownTimer) {
          clearInterval(cooldownTimer)
          cooldownTimer = null
        }
      }
    }, 1000)
  } catch (e) {
    // 后端字数硬限制等错误透传到这里（invoke reject）
    notifyError.value = (e as Error)?.message || String(e)
  }
}

// 复制 curl 命令
const CURL_TEXT = `curl -X POST http://127.0.0.1:8756/notify \\
  -H 'Content-Type: application/json' \\
  -d '{"text":"下班啦！","action":"idle"}'`
const curlCopied = ref(false)
let curlCopiedTimer: ReturnType<typeof setTimeout> | null = null
async function copyCurl(): Promise<void> {
  try {
    await navigator.clipboard.writeText(CURL_TEXT)
  } catch {
    // 剪贴板不可用时回退：用临时 textarea 复制
    const ta = document.createElement('textarea')
    ta.value = CURL_TEXT
    document.body.appendChild(ta)
    ta.select()
    document.execCommand('copy')
    document.body.removeChild(ta)
  }
  curlCopied.value = true
  if (curlCopiedTimer) clearTimeout(curlCopiedTimer)
  curlCopiedTimer = setTimeout(() => {
    curlCopied.value = false
  }, 1500)
}

// ── 添加外部宠物 ──
const fileInput = ref<HTMLInputElement | null>(null)
const importing = ref(false)
const importError = ref('')

function onPickFile(): void {
  fileInput.value?.click()
}

// ── 在线画廊 ──
const galleryOpen = ref(false)
const galleryLoading = ref(false)
const galleryError = ref('')
const onlinePets = ref<OnlinePetMeta[]>([])
const galleryKeyword = ref('')
// 正在下载的 slug 集合（用于按钮 loading 态）
const downloading = ref<Record<string, boolean>>({})

const filteredOnlinePets = computed<OnlinePetMeta[]>(() => {
  const kw = galleryKeyword.value.trim().toLowerCase()
  if (!kw) return onlinePets.value
  return onlinePets.value.filter(
    (p) =>
      p.name.toLowerCase().includes(kw) ||
      p.author.toLowerCase().includes(kw) ||
      p.category.toLowerCase().includes(kw),
  )
})

// 已下载到本地的 slug 集合（避免重复下载）
const installedSlugs = computed<Set<string>>(() => new Set(petStore.pets.map((p) => p.id)))

function openGallery(): void {
  galleryOpen.value = true
  void loadGallery()
}
function closeGallery(): void {
  galleryOpen.value = false
}
async function loadGallery(): Promise<void> {
  if (galleryLoading.value) return
  galleryLoading.value = true
  galleryError.value = ''
  try {
    const list = await browseOnlinePets()
    onlinePets.value = list
    if (!list.length) galleryError.value = '画廊为空或加载失败'
  } catch (e) {
    galleryError.value = `加载画廊失败：${(e as Error)?.message || e}`
  } finally {
    galleryLoading.value = false
  }
}
async function downloadPet(p: OnlinePetMeta): Promise<void> {
  if (downloading.value[p.slug]) return
  downloading.value = { ...downloading.value, [p.slug]: true }
  try {
    const def = (await downloadOnlinePet(p.slug)) as {
      id: string
      display_name: string
      description: string
      spritesheet: string
      idle: PetDef['idle']
      talk: PetDef['talk']
      actions: Record<string, PetDef['idle']>
    }
    const pet = registerDownloadedPet(def)
    // 下载成功后自动切换到该宠物（跨窗口广播，main 宠物窗口实时生效）
    setCurrentPet(pet.id)
    pushNotify(`已下载并切换到宠物「${def.display_name}」`, 'wave')
  } catch (e) {
    pushNotify(`下载失败：${(e as Error)?.message || e}`)
  } finally {
    downloading.value = { ...downloading.value, [p.slug]: false }
  }
}
function arrayBufferToBase64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf)
  let binary = ''
  const chunkSize = 0x8000
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize))
  }
  return btoa(binary)
}
async function onFileChosen(e: Event): Promise<void> {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  if (!/\.zip$/i.test(file.name)) {
    importError.value = '仅支持 .zip 压缩包'
    return
  }
  importing.value = true
  importError.value = ''
  try {
    const buf = await file.arrayBuffer()
    const base64 = arrayBufferToBase64(buf)
    const pet = await importExternalPet(base64)
    setCurrentPet(pet.id)
    pushNotify(`宠物「${pet.displayName}」导入成功！`, 'wave')
  } catch (err) {
    importError.value = err instanceof Error ? err.message : String(err)
  } finally {
    importing.value = false
  }
}

function onClose(): void {
  void closeSettingsWindow()
}

// 拖动窗口（Tauri v2 用 startDragging API）。
// 绑在整页 .settings-root 上，任意空白处都可拖（含顶部 16px padding 带）。
// 关键：不在 mousedown 内同步 startDragging（Windows 上会吞掉 click，导致列表项/开关点不动），
// 而是「鼠标移动超过阈值才真正拖拽」——与 PetHost 的阈值机制一致。
// 命中交互元素（按钮/输入框/滑块/卡片等）不启动拖拽，保证点击/滑动不被吞。
const DRAG_THRESHOLD_PX = 5
let sDragStartX = 0
let sDragStartY = 0
let sDragMoved = false
let sDragging = false

function onRootMouseDown(e: MouseEvent): void {
  if (e.button !== 0) return
  const target = e.target as HTMLElement
  // 交互元素不启动窗口拖拽
  if (
    target.closest(
      'button, a, input, textarea, select, .s-slider, .s-card, .s-gallery-card, .s-item, .s-toggle, .s-modal, .s-editpet, .s-gallery',
    )
  ) {
    return
  }
  sDragging = true
  sDragMoved = false
  sDragStartX = e.clientX
  sDragStartY = e.clientY
  window.addEventListener('mousemove', onRootDragMove)
  window.addEventListener('mouseup', onRootMouseUp)
}

function onRootDragMove(e: MouseEvent): void {
  if (!sDragging) return
  const dx = e.clientX - sDragStartX
  const dy = e.clientY - sDragStartY
  if (!sDragMoved && Math.abs(dx) < DRAG_THRESHOLD_PX && Math.abs(dy) < DRAG_THRESHOLD_PX) {
    return
  }
  if (!sDragMoved) {
    sDragMoved = true
    void startDragging()
  }
}

function onRootMouseUp(): void {
  sDragging = false
  window.removeEventListener('mousemove', onRootDragMove)
  window.removeEventListener('mouseup', onRootMouseUp)
}
</script>

<template>
  <div class="settings-root" @mousedown="onRootMouseDown">
    <div class="s-header">
      <span class="s-brand">
        <svg
          class="s-brand-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <defs>
            <linearGradient id="brand-grad" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stop-color="var(--primary, #3b6ef5)" />
              <stop offset="100%" stop-color="#8a5cf6" />
            </linearGradient>
          </defs>
          <path d="M5 14a7 7 0 0 1 14 0" stroke="url(#brand-grad)" />
          <circle cx="9" cy="11.5" r="1" fill="url(#brand-grad)" stroke="none" />
          <circle cx="15" cy="11.5" r="1" fill="url(#brand-grad)" stroke="none" />
          <path d="M9.5 16c1 .8 4 .8 5 0" stroke="url(#brand-grad)" />
          <path d="M12 3.5c-.6-1-.2-2 .8-2s1.4.9.8 1.8" stroke="url(#brand-grad)" />
          <path d="M9 4c-.5-1 .2-2 1.1-1.8.6.1.7 1 .3 1.8" stroke="url(#brand-grad)" />
        </svg>
        <span class="s-title">PetBuddy 设置</span>
      </span>
      <button class="s-close" aria-label="关闭" @click="onClose"></button>
    </div>

    <div class="s-body">
      <!-- 左侧：预览 + 缩放 + 显示/测试 -->
      <div class="s-left">
        <div class="s-preview">
          <SpritePet :state="'idle'" :scale="1" />
          <div class="s-preview-name">{{ currentPet?.displayName ?? '—' }}</div>
        </div>

        <!-- 缩放 -->
        <div class="s-section">
          <div class="s-label-row">
            <span class="s-label">缩放大小</span>
            <span class="s-scale-val">{{ Math.round(petStore.scale * 100) }}%</span>
          </div>
          <div
            ref="trackEl"
            class="s-slider"
            @pointerdown="onTrackPointerDown"
            @pointermove="onTrackPointerMove"
            @pointerup="onTrackPointerUp"
            @pointercancel="onTrackPointerUp"
          >
            <div class="s-slider-fill" :style="{ width: scaleToPercent(petStore.scale) + '%' }" />
            <div class="s-slider-thumb" :style="{ left: scaleToPercent(petStore.scale) + '%' }" />
          </div>
        </div>

        <!-- 显示宠物 + 测试通知 -->
        <div class="s-section s-actions">
          <label class="s-toggle" title="显示 / 隐藏桌面宠物">
            <input type="checkbox" :checked="petStore.visible" @change="onToggleVisible" />
            <span class="s-toggle-track"><span class="s-toggle-thumb" /></span>
            <span class="s-toggle-text">显示宠物</span>
          </label>
          <button class="s-test-btn" @click="openNotifyModal">测试通知</button>
        </div>

        <!-- 左下角版本号 -->
        <div class="s-version">PetBuddy v{{ appVersion || '…' }}</div>
      </div>

      <!-- 右侧：宠物列表 + 添加 -->
      <div class="s-right">
        <div class="s-label">选择宠物</div>
        <div class="s-list">
          <div
            v-for="p in petStore.pets"
            :key="p.id"
            class="s-item"
            :class="{ active: p.id === petStore.currentId }"
            @click="onSelect(p.id)"
          >
            <div class="s-item-main">
              <span class="s-item-name">{{ p.displayName }}</span>
              <span class="s-item-desc">{{ p.description }}</span>
            </div>
            <button
              v-if="p.external"
              class="s-item-edit"
              title="编辑宠物信息"
              @click.stop="openEditPet(p)"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              >
                <path d="M12 20h9" />
                <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
              </svg>
            </button>
            <button
              v-if="p.external"
              class="s-item-del"
              :class="{ confirm: pendingDeleteId === p.id }"
              :title="pendingDeleteId === p.id ? '再次点击确认删除' : '删除该宠物'"
              @click.stop="onDeleteClick(p)"
            >
              <svg
                v-if="pendingDeleteId !== p.id"
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              >
                <path d="M3 6h18" />
                <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
              </svg>
              <span v-else class="s-del-confirm-text">确认</span>
            </button>
          </div>
        </div>

        <div class="s-add-row">
          <button class="s-add" :disabled="importing" @click="onPickFile">
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <path d="M12 5v14" />
              <path d="M5 12h14" />
            </svg>
            <span>{{ importing ? '导入中…' : '本地导入' }}</span>
          </button>
          <button class="s-add s-add-online" :disabled="importing" @click="openGallery">
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
            >
              <circle cx="12" cy="12" r="9" />
              <path d="M3 12h18" />
              <path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18" />
            </svg>
            <span>在线画廊</span>
          </button>
        </div>
        <div v-if="importError" class="s-import-error">{{ importError }}</div>
        <input
          ref="fileInput"
          type="file"
          accept=".zip"
          style="display: none"
          @change="onFileChosen"
        />
      </div>
    </div>

    <!-- 在线画廊弹窗 -->
    <div v-if="galleryOpen" class="s-gallery-mask" @click.self="closeGallery">
      <div class="s-gallery">
        <div class="s-gallery-head">
          <div class="s-gallery-head-text">
            <span class="s-gallery-title">在线画廊</span>
            <span class="s-gallery-source">
              数据来源：
              <a
                class="s-gallery-link"
                href="https://github.com/legeling/awesome-codex-pet"
                @click.prevent="openExternal('https://github.com/legeling/awesome-codex-pet')"
                >awesome-codex-pet（GitHub 开源仓库）</a
              >
              · 预览图由
              <a
                class="s-gallery-link"
                href="https://codexpet.top"
                @click.prevent="openExternal('https://codexpet.top')"
                >codexpet.top</a
              >
              提供
            </span>
          </div>
          <button class="s-close" aria-label="关闭" @click="closeGallery"></button>
        </div>
        <div class="s-gallery-search">
          <div class="s-gallery-search-wrap">
            <input
              v-model="galleryKeyword"
              class="s-gallery-search-input"
              type="text"
              placeholder="搜索名字 / 作者 / 分类"
            />
            <button
              v-if="galleryKeyword"
              class="s-gallery-clear"
              title="清空搜索"
              aria-label="清空搜索"
              @click="galleryKeyword = ''"
            >
              <svg
                width="10"
                height="10"
                viewBox="0 0 10 10"
                fill="none"
                stroke="currentColor"
                stroke-width="1.6"
                stroke-linecap="round"
              >
                <line x1="2" y1="2" x2="8" y2="8" />
                <line x1="8" y1="2" x2="2" y2="8" />
              </svg>
            </button>
          </div>
          <button class="s-gallery-refresh" :disabled="galleryLoading" @click="loadGallery">
            {{ galleryLoading ? '加载中…' : '刷新' }}
          </button>
        </div>
        <div class="s-gallery-body">
          <div v-if="galleryLoading && !onlinePets.length" class="s-gallery-placeholder">
            画廊加载中…
          </div>
          <div v-else-if="galleryError" class="s-gallery-placeholder">{{ galleryError }}</div>
          <div v-else-if="!filteredOnlinePets.length" class="s-gallery-placeholder">
            没有匹配的宠物
          </div>
          <div v-else class="s-gallery-grid">
            <div v-for="p in filteredOnlinePets" :key="p.slug" class="s-gallery-card">
              <div class="s-gallery-thumb">
                <img
                  :src="p.preview_url"
                  :alt="p.name"
                  loading="lazy"
                  class="s-gallery-img"
                  @error="($event.target as HTMLImageElement).style.visibility = 'hidden'"
                />
              </div>
              <div class="s-gallery-card-top">
                <span class="s-gallery-card-name">{{ p.name }}</span>
                <span class="s-gallery-ver">v{{ p.sprite_version }}</span>
              </div>
              <div class="s-gallery-card-meta">{{ p.author }} · {{ p.category }}</div>
              <div class="s-gallery-card-slug">ID: {{ p.slug }}</div>
              <button
                class="s-gallery-dl"
                :class="{ 'is-installed': installedSlugs.has(p.slug) && !downloading[p.slug] }"
                :disabled="downloading[p.slug]"
                @click="downloadPet(p)"
              >
                <template v-if="downloading[p.slug]">下载中…</template>
                <template v-else-if="installedSlugs.has(p.slug)">重新下载</template>
                <template v-else>下载</template>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 测试通知弹窗（自定义内容） -->
    <div v-if="notifyModalOpen" class="s-modal-mask" @click.self="closeNotifyModal">
      <div class="s-modal">
        <div class="s-modal-head">
          <span class="s-modal-title">发送测试通知</span>
          <button class="s-close" aria-label="关闭" @click="closeNotifyModal"></button>
        </div>
        <div class="s-modal-body">
          <div class="s-field-label">通知内容</div>
          <div class="s-input-wrap">
            <textarea
              v-model="notifyText"
              class="s-modal-input"
              rows="3"
              maxlength="120"
              placeholder="输入要显示的通知文字…"
              @keydown.meta.enter="sendNotify"
              @keydown.ctrl.enter="sendNotify"
            />
            <div class="s-char-count">{{ notifyText.length }}/120</div>
            <div v-if="notifyError" class="s-char-error">{{ notifyError }}</div>
          </div>

          <div class="s-field-label s-tips-label">调用教程</div>
          <div class="s-modal-tips">
            <div class="s-tips-sub">HTTP 接口（任意外部程序，端口 8756）</div>
            <div class="s-code-wrap">
              <pre class="s-code">
curl -X POST http://127.0.0.1:8756/notify \
  -H 'Content-Type: application/json' \
  -d '{"text":"下班啦！","action":"idle"}'</pre>
              <button class="s-copy-btn" :class="{ copied: curlCopied }" @click="copyCurl">
                {{ curlCopied ? '已复制' : '复制' }}
              </button>
            </div>
            <div class="s-tips-note">
              提示：宠物需处于「显示」状态才能看到气泡；气泡默认 4
              秒后自动消失，多条通知会依次排队播放。
            </div>
          </div>
        </div>
        <div class="s-modal-foot">
          <button class="s-modal-cancel" @click="closeNotifyModal">取消</button>
          <button
            class="s-modal-send"
            :disabled="!notifyText.trim() || cooldownLeft > 0"
            @click="sendNotify"
          >
            {{ cooldownLeft > 0 ? `冷却中 ${cooldownLeft}s` : '发送通知' }}
          </button>
        </div>
      </div>
    </div>
    <!-- 编辑外部宠物弹窗 -->
    <div v-if="editPetOpen" class="s-editpet-mask" @click.self="closeEditPet">
      <div class="s-editpet">
        <div class="s-editpet-head">
          <span class="s-editpet-title">编辑宠物信息</span>
          <button class="s-close" aria-label="关闭" @click="closeEditPet"></button>
        </div>
        <div class="s-editpet-body">
          <label class="s-editpet-field">
            <span class="s-editpet-label">名字</span>
            <input
              v-model="editPetName"
              class="s-editpet-input"
              type="text"
              maxlength="40"
              placeholder="宠物显示名"
            />
          </label>
          <label class="s-editpet-field">
            <span class="s-editpet-label">描述</span>
            <textarea
              v-model="editPetDesc"
              class="s-editpet-textarea"
              rows="3"
              maxlength="200"
              placeholder="宠物描述（可选）"
            ></textarea>
          </label>
          <div v-if="editPetError" class="s-editpet-error">{{ editPetError }}</div>
        </div>
        <div class="s-editpet-foot">
          <button class="s-editpet-cancel" @click="closeEditPet">取消</button>
          <button class="s-editpet-save" :disabled="editPetSaving" @click="saveEditPet">
            {{ editPetSaving ? '保存中…' : '保存' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-root {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg);
  /* 左右下留边距给内容；顶部 padding 改为 0，避免「16px 直角背景带顶到圆角切口」的观感冲突。
     顶部留白由 .s-header 的 margin-top 提供，根节点顶部直接是圆角切口。 */
  padding: 0 20px 20px;
  /* 无边框透明窗口：CSS 自绘圆角（与气泡统一用 --radius-window，保证两端圆角一致） */
  border-radius: var(--radius-window);
  overflow: hidden;
  /* 禁止选中文字（避免拖动缩放滑块时误选中文本） */
  user-select: none;
  -webkit-user-select: none;
}
.s-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 14px;
  margin-bottom: 14px;
  flex-shrink: 0;
  user-select: none;
  -webkit-user-select: none;
}
.s-brand {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.s-brand-icon {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
  /* SVG 自带渐变描边/填充，外部仅加柔光 */
  filter: drop-shadow(0 1px 3px rgba(59, 110, 245, 0.35));
}
.s-title {
  font-size: 16px;
  font-weight: 700;
  letter-spacing: 0.4px;
  background: linear-gradient(135deg, var(--primary, #3b6ef5), #8a5cf6);
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
  filter: drop-shadow(0 1px 2px rgba(59, 110, 245, 0.18));
}
.s-close {
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--muted);
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
  /* 用两条旋转线画叉号，脱离字体度量，跨平台（尤其 Windows）精确居中，
     避免文字 × 因字体基线偏移导致在 Windows 上明显偏下 */
}
.s-close::before,
.s-close::after {
  content: '';
  position: absolute;
  left: 50%;
  top: 50%;
  width: 14px;
  height: 2px;
  border-radius: 2px;
  background: currentColor;
}
.s-close::before {
  transform: translate(-50%, -50%) rotate(45deg);
}
.s-close::after {
  transform: translate(-50%, -50%) rotate(-45deg);
}
.s-close:hover {
  background: var(--danger-soft);
  color: var(--danger);
}
.s-body {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: stretch;
  gap: 4px;
}
.s-left {
  flex: 0 0 260px;
  display: flex;
  flex-direction: column;
}
.s-right {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding-left: 20px;
}
.s-preview {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 16px;
  min-height: 220px;
  background: linear-gradient(160deg, var(--primary-soft), rgba(255, 255, 255, 0));
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  margin-bottom: 16px;
}
.s-preview-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}
.s-section {
  margin-bottom: 16px;
}
.s-label-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.s-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--muted);
  margin-bottom: 10px;
}
.s-scale-val {
  font-size: 12px;
  font-weight: 700;
  color: var(--primary);
  background: var(--primary-soft);
  padding: 2px 8px;
  border-radius: var(--radius-pill);
}
.s-slider {
  position: relative;
  width: 100%;
  height: 20px;
  display: flex;
  align-items: center;
  cursor: pointer;
  touch-action: none;
  user-select: none;
}
.s-slider::before {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  height: 6px;
  border-radius: 999px;
  background: #e3e6ec;
}
.s-slider-fill {
  position: absolute;
  left: 0;
  height: 6px;
  border-radius: 999px;
  background: var(--primary);
  pointer-events: none;
}
.s-slider-thumb {
  position: absolute;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--primary);
  border: 2px solid #fff;
  box-shadow: 0 1px 4px rgba(31, 39, 51, 0.28);
  transform: translateX(-50%);
  pointer-events: none;
}
.s-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.s-actions .s-test-btn {
  margin-left: auto;
}
.s-toggle {
  display: inline-flex;
  align-items: center;
  gap: 9px;
  cursor: pointer;
  user-select: none;
}
.s-toggle input {
  display: none;
}
.s-toggle-track {
  position: relative;
  width: 40px;
  height: 22px;
  border-radius: 999px;
  background: rgba(31, 39, 51, 0.18);
  transition: background 0.2s ease;
  flex-shrink: 0;
}
.s-toggle-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(31, 39, 51, 0.3);
  transition: transform 0.2s cubic-bezier(0.22, 1, 0.36, 1);
}
.s-toggle input:checked + .s-toggle-track {
  background: var(--primary);
}
.s-toggle input:checked + .s-toggle-track .s-toggle-thumb {
  transform: translateX(18px);
}
.s-toggle-text {
  font-size: 13px;
  font-weight: 500;
  color: var(--text);
}
.s-test-btn {
  border: 1px solid transparent;
  background: linear-gradient(160deg, var(--primary), var(--primary-hover));
  color: #fff;
  font-size: 12px;
  font-weight: 600;
  padding: 7px 14px;
  border-radius: var(--radius-pill);
  cursor: pointer;
  box-shadow: 0 6px 16px rgba(59, 110, 245, 0.28);
  transition:
    filter 0.18s ease,
    transform 0.12s ease;
}
.s-test-btn:hover {
  filter: brightness(1.06);
}
.s-test-btn:active {
  transform: scale(0.97);
}
.s-version {
  margin-top: auto;
  padding-top: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  font-size: 12px;
  font-weight: 600;
  font-family: 'SF Mono', 'JetBrains Mono', 'Fira Code', ui-monospace, Menlo, Consolas, monospace;
  letter-spacing: 1px;
  background: linear-gradient(135deg, var(--primary, #3b6ef5), #8a5cf6);
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
  text-shadow: 0 1px 1px rgba(255, 255, 255, 0.25);
  filter: drop-shadow(0 1px 2px rgba(59, 110, 245, 0.18));
}
/* 版本号前的小圆点，增强质感 */
.s-version::before {
  content: '';
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--primary, #3b6ef5), #8a5cf6);
  box-shadow: 0 0 6px rgba(59, 110, 245, 0.55);
}
.s-list {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow-y: auto;
  padding-right: 2px;
}
.s-item {
  text-align: left;
  border: 1px solid var(--border);
  background: transparent;
  border-radius: var(--radius-sm);
  padding: 10px 13px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 10px;
  transition: all 0.18s ease;
}
.s-item:hover {
  border-color: var(--primary);
  background: var(--primary-soft);
}
.s-item.active {
  border-color: var(--primary);
  background: var(--primary-soft);
  box-shadow: inset 0 0 0 1px var(--primary-soft);
}
.s-item-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.s-item-del {
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--muted);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s ease;
}
.s-item-del:hover {
  background: var(--danger-soft);
  color: var(--danger);
}
.s-item-del.confirm {
  width: auto;
  padding: 0 8px;
  border-radius: 999px;
  background: var(--danger);
  color: #fff;
}
.s-item-del.confirm:hover {
  background: var(--danger);
  color: #fff;
}
.s-item-edit {
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--muted);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s ease;
}
.s-item-edit:hover {
  background: rgba(31, 39, 51, 0.08);
  color: var(--text);
}
.s-del-confirm-text {
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}
.s-item-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}
.s-item-desc {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.4;
}
.s-add-row {
  margin-top: 12px;
  display: flex;
  gap: 8px;
}
.s-add {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  padding: 11px;
  border: 1.5px dashed var(--border-strong);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--muted);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.18s ease;
}
.s-add:hover {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--primary-soft);
}
.s-add-online {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--primary-soft);
}
.s-add-online:hover {
  border-color: var(--primary);
  color: #fff;
  background: var(--primary);
}
.s-add:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.s-import-error {
  margin-top: 8px;
  font-size: 11px;
  color: var(--danger);
}
.s-gallery-mask {
  position: absolute;
  inset: 0;
  background: rgba(15, 23, 42, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 50;
  /* 跟随窗口圆角（同 .s-modal-mask），避免深色蒙版溢出圆角外 */
  border-radius: var(--radius-window);
  overflow: hidden;
}
.s-modal-mask {
  position: absolute;
  inset: 0;
  background: rgba(15, 23, 42, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 60;
  /* 跟随窗口圆角：蒙版本身是直角矩形铺满根，加上圆角+裁切后四角与窗口一致，
     避免半透明深色溢出到圆角外的透明区域（「蒙版遮挡四角」问题）。 */
  border-radius: var(--radius-window);
  overflow: hidden;
}
.s-modal {
  width: 92%;
  max-width: 460px;
  max-height: 90%;
  display: flex;
  /* 弹窗圆角统一 8px（与设置窗/蒙版一致） */
  flex-direction: column;
  background: var(--panel, #fff);
  border-radius: 8px;
  box-shadow: 0 20px 50px rgba(0, 0, 0, 0.3);
  overflow: hidden;
}
.s-modal-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 20px;
  border-bottom: 1px solid var(--border, #eee);
  flex-shrink: 0;
}
.s-modal-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
}
.s-modal-body {
  padding: 16px 20px;
  overflow-y: auto;
}
.s-field-label {
  display: block;
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
  margin: 4px 0 6px;
}
.s-tips-label {
  margin-top: 16px;
}
.s-modal-input {
  width: 100%;
  padding: 9px 11px 22px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: var(--radius-sm, 8px);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.5;
  outline: none;
  background: var(--panel, #fff);
  color: var(--text);
  resize: none;
  box-sizing: border-box;
  -webkit-user-drag: none;
}
.s-modal-input:focus {
  border-color: var(--primary);
}

/* 编辑外部宠物弹窗 */
.s-editpet-mask {
  position: absolute;
  inset: 0;
  background: rgba(15, 23, 42, 0.35);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 50;
  /* 跟随窗口圆角（同 .s-modal-mask），避免深色蒙版溢出圆角外 */
  border-radius: var(--radius-window);
  overflow: hidden;
}
.s-editpet {
  width: 90%;
  max-width: 380px;
  background: var(--panel, #fff);
  border-radius: 8px;
  box-shadow: 0 20px 50px rgba(0, 0, 0, 0.3);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.s-editpet-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  border-bottom: 1px solid var(--border, #eee);
}
.s-editpet-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text);
}
.s-editpet-body {
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.s-editpet-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.s-editpet-label {
  font-size: 12px;
  color: var(--muted);
}
.s-editpet-input,
.s-editpet-textarea {
  width: 100%;
  padding: 9px 11px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: 8px;
  font-size: 13px;
  color: var(--text);
  font-family: inherit;
  resize: none;
  box-sizing: border-box;
  background: var(--panel, #fff);
}
.s-editpet-input:focus,
.s-editpet-textarea:focus {
  border-color: var(--primary);
  outline: none;
}
.s-editpet-error {
  font-size: 12px;
  color: #e5484d;
}
.s-editpet-foot {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 12px 18px;
  border-top: 1px solid var(--border, #eee);
}
.s-editpet-cancel {
  padding: 7px 16px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: 999px;
  background: transparent;
  color: var(--text);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}
.s-editpet-cancel:hover {
  background: rgba(31, 39, 51, 0.05);
}
.s-editpet-save {
  padding: 7px 18px;
  border: none;
  border-radius: 999px;
  background: var(--primary);
  color: #fff;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s ease;
}
.s-editpet-save:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.s-editpet-save:not(:disabled):hover {
  filter: brightness(0.95);
}
.s-input-wrap {
  position: relative;
}
.s-char-count {
  position: absolute;
  right: 8px;
  bottom: 9px;
  font-size: 11px;
  color: var(--muted);
  pointer-events: none;
  user-select: none;
}
.s-char-error {
  margin-top: 4px;
  font-size: 12px;
  color: #e5484d;
  line-height: 1.4;
}
.s-modal-tips {
  margin-top: 8px;
  padding: 12px;
  background: var(--panel-2, rgba(241, 243, 247, 0.7));
  border-radius: var(--radius-sm, 8px);
}
.s-tips-sub {
  font-size: 11px;
  color: var(--muted);
  margin: 0 0 4px;
}
.s-code-wrap {
  position: relative;
}
.s-code {
  margin: 0;
  padding: 8px 52px 8px 10px;
  background: rgba(31, 39, 51, 0.06);
  border-radius: 6px;
  font-size: 11px;
  line-height: 1.55;
  color: var(--text);
  white-space: pre-wrap;
  word-break: break-all;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
.s-copy-btn {
  position: absolute;
  top: 6px;
  right: 6px;
  padding: 3px 8px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: 5px;
  background: var(--panel, #fff);
  color: var(--muted);
  font-size: 11px;
  cursor: pointer;
  line-height: 1;
}
.s-copy-btn:hover {
  border-color: var(--primary);
  color: var(--primary);
}
.s-copy-btn.copied {
  border-color: var(--primary);
  color: var(--primary);
}
.s-tips-note {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.5;
  margin-top: 8px;
}
.s-modal-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 20px;
  border-top: 1px solid var(--border, #eee);
  flex-shrink: 0;
}
.s-modal-cancel,
.s-modal-send {
  padding: 7px 16px;
  border-radius: var(--radius-sm, 8px);
  font-size: 13px;
  cursor: pointer;
  border: 1px solid transparent;
}
.s-modal-cancel {
  background: transparent;
  border-color: var(--border-strong, #ddd);
  color: var(--text);
}
.s-modal-cancel:hover {
  background: rgba(31, 39, 51, 0.05);
}
.s-modal-send {
  background: var(--primary);
  color: #fff;
}
.s-modal-send:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.s-modal-send:not(:disabled):hover {
  filter: brightness(0.95);
}
.s-gallery {
  width: 94%;
  max-width: 900px;
  height: 88%;
  display: flex;
  flex-direction: column;
  background: var(--panel, #fff);
  border-radius: 8px;
  box-shadow: 0 20px 50px rgba(0, 0, 0, 0.3);
  overflow: hidden;
}
.s-gallery-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border, #eee);
}
.s-gallery-head-text {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}
.s-gallery-title {
  font-size: 17px;
  font-weight: 700;
  color: var(--text);
  white-space: nowrap;
}
.s-gallery-source {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.4;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.s-gallery-link {
  color: var(--muted);
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: pointer;
}
.s-gallery-link:hover {
  color: var(--primary);
}
.s-gallery-body {
  flex: 1;
  min-height: 0;
  padding: 20px;
  overflow-y: auto;
}
.s-gallery-placeholder {
  text-align: center;
  color: var(--muted);
  font-size: 13px;
  padding: 40px 0;
}
.s-gallery-search {
  display: flex;
  gap: 8px;
  padding: 12px 20px;
  border-bottom: 1px solid var(--border, #eee);
}
.s-gallery-search-wrap {
  position: relative;
  flex: 1;
  display: flex;
  align-items: center;
}
.s-gallery-search-input {
  flex: 1;
  padding: 8px 28px 8px 12px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: var(--radius-sm, 8px);
  font-size: 13px;
  line-height: 1.4;
  outline: none;
  background: var(--panel, #fff);
  color: var(--text);
}
.s-gallery-search-input:focus {
  border-color: var(--primary);
}
.s-gallery-clear {
  position: absolute;
  right: 9px;
  top: 50%;
  width: 18px;
  height: 18px;
  margin-top: -9px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: rgba(31, 39, 51, 0.06);
  color: var(--muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition:
    background 0.15s ease,
    color 0.15s ease;
}
.s-gallery-clear:hover {
  background: rgba(31, 39, 51, 0.12);
  color: var(--muted);
}
.s-gallery-refresh {
  flex: none;
  min-width: 8em;
  padding: 8px 6px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: var(--radius-sm, 8px);
  background: transparent;
  color: var(--muted);
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
  text-align: center;
  box-sizing: border-box;
}
.s-gallery-refresh:hover:not(:disabled) {
  border-color: var(--primary);
  color: var(--primary);
}
.s-gallery-refresh:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.s-gallery-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 14px;
}
.s-gallery-card {
  border: 1px solid var(--border, #eee);
  border-radius: var(--radius-sm, 8px);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  background: var(--panel, #fff);
}
.s-gallery-thumb {
  height: 120px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--primary-soft, #f3f4ff);
}
.s-gallery-img {
  max-width: 100%;
  max-height: 100%;
}
.s-gallery-card-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  padding: 10px 12px 0;
}
.s-gallery-card-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.s-gallery-ver {
  flex-shrink: 0;
  font-size: 10px;
  font-weight: 600;
  color: var(--primary);
  background: var(--primary-soft, #f3f4ff);
  border-radius: 4px;
  padding: 1px 6px;
}
.s-gallery-card-meta {
  padding: 3px 12px 0;
  font-size: 11px;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.s-gallery-card-slug {
  padding: 4px 12px 0;
  font-size: 10px;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.s-gallery-dl {
  margin: 10px 12px 12px;
  padding: 8px;
  border: none;
  border-radius: var(--radius-sm, 8px);
  background: var(--primary);
  color: #fff;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s ease;
}
.s-gallery-dl:hover:not(:disabled) {
  opacity: 0.9;
}
.s-gallery-dl:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
/* 已下载（非下载中）：描边样式，明确「可重复下载」 */
.s-gallery-dl.is-installed {
  background: transparent;
  color: var(--primary);
  border: 1px solid var(--primary);
}
.s-gallery-dl.is-installed:hover:not(:disabled) {
  background: var(--primary-soft, #f3f4ff);
  opacity: 1;
}
</style>
