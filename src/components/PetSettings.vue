<script setup lang="ts">
// 宠物设置页（settings 窗口整页内容）。
import { onMounted, ref, computed } from 'vue'
import { petStore, currentPet, setCurrentPet, setPetScale, setPetVisible, importExternalPet, deleteExternalPet, registerDownloadedPet, loadPetManifest, type PetDef } from '../store/pet'
import { pushNotify } from '../store/notify'
import { closeSettingsWindow, startDragging, browseOnlinePets, downloadOnlinePet, openExternal, preloadTauri, emitEvent, onEvent, isTauri, type OnlinePetMeta } from '../tauri'
import SpritePet from './SpritePet.vue'

// settings 窗口是独立 webview，pets 由 App.vue onMounted 异步加载；
// 此处兜底：挂载时若尚未加载则主动加载，保证列表与预览始终有数据。
// 同时主动从 main 窗口同步「当前选中的宠物」，避免首屏先闪第一个宠物。
onMounted(() => {
  // settings 是独立 webview 窗口，必须在此预加载 windowApi，
  // 否则 startDragging 因 windowApi 为 null 而直接 return，窗口无法拖动。
  void preloadTauri()
  // 监听 main 广播的宠物切换，实时同步当前选中（main 打开设置时会回复当前选中）。
  if (isTauri) {
    void onEvent('pet-switch', (payload) => {
      const raw = String(payload)
      const pureId = raw.startsWith('pet:') ? raw.slice(4) : raw
      if (pureId && petStore.pets.some((p) => p.id === pureId)) {
        petStore.currentId = pureId
      }
    })
    // 向 main 请求当前选中的宠物（main 的 PetHost 收到后会 emit pet-switch 带当前 id）。
    void emitEvent('request-current-pet', '')
  }
  if (!petStore.pets.length) {
    void loadPetManifest()
  }
  // 禁用右键菜单（避免无边框窗口里弹出 webview 默认菜单）
  document.addEventListener('contextmenu', (e) => e.preventDefault())
})

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

// ── 缩放滑块（自绘：div 轨道 + 填充 + 圆点，像素级对齐）──
const MIN_SCALE = 0.8
const MAX_SCALE = 1.3
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
const notifyAction = ref('')
const NOTIFY_ACTIONS: Array<{ value: string; label: string }> = [
  { value: '', label: '无（默认 idle）' },
  { value: 'wave', label: '招手 wave' },
  { value: 'jump', label: '跳跃 jump' },
  { value: 'failed', label: '失败 failed' },
  { value: 'working', label: '工作 working' },
  { value: 'waiting', label: '等待 waiting' },
  { value: 'look', label: '张望 look' },
  { value: 'run', label: '跑步 run' },
]
function openNotifyModal(): void {
  notifyText.value = ''
  notifyAction.value = ''
  notifyModalOpen.value = true
}
function closeNotifyModal(): void {
  notifyModalOpen.value = false
}
function sendNotify(): void {
  const text = notifyText.value.trim()
  if (!text) return
  pushNotify(text, notifyAction.value || undefined)
  closeNotifyModal()
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
const installedSlugs = computed<Set<string>>(
  () => new Set(petStore.pets.map((p) => p.id)),
)

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
    registerDownloadedPet(def)
    pushNotify(`已下载宠物「${def.display_name}」`)
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
    const pet = await importExternalPet(base64, file.name)
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

// 拖动窗口（Tauri v2 用 startDragging API，必须在 mousedown 内同步调用）。
// 绑在 .settings-root 上，包含 header 与 root 的 padding 区域，使整页背景都可拖。
// 跳过交互元素，避免拖动吞掉点击/拖动滑块等手势。
function onRootMouseDown(e: MouseEvent): void {
  const target = e.target as HTMLElement
  // 仅响应主鼠标按键
  if (e.button !== 0) return
  // 命中以下交互元素则不启动窗口拖拽
  if (target.closest('button, a, input, textarea, select, .s-slider, .s-card, .s-gallery-card')) return
  void startDragging()
}
</script>

<template>
  <div class="settings-root" @mousedown="onRootMouseDown">
    <div class="s-header">
      <span class="s-title">PetBuddy 设置</span>
      <button class="s-close" @click="onClose">×</button>
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
              class="s-item-del"
              :class="{ confirm: pendingDeleteId === p.id }"
              :title="pendingDeleteId === p.id ? '再次点击确认删除' : '删除该宠物'"
              @click.stop="onDeleteClick(p)"
            >
              <svg v-if="pendingDeleteId !== p.id" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
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
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M12 5v14" />
              <path d="M5 12h14" />
            </svg>
            <span>{{ importing ? '导入中…' : '本地导入' }}</span>
          </button>
          <button class="s-add s-add-online" :disabled="importing" @click="openGallery">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="9" />
              <path d="M3 12h18" />
              <path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18" />
            </svg>
            <span>在线画廊</span>
          </button>
        </div>
        <div v-if="importError" class="s-import-error">{{ importError }}</div>
        <input ref="fileInput" type="file" accept=".zip" style="display: none" @change="onFileChosen" />
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
              <a class="s-gallery-link" href="https://github.com/legeling/awesome-codex-pet" @click.prevent="openExternal('https://github.com/legeling/awesome-codex-pet')">awesome-codex-pet（GitHub 开源仓库）</a>
              · 预览图由
              <a class="s-gallery-link" href="https://codexpet.top" @click.prevent="openExternal('https://codexpet.top')">codexpet.top</a>
              提供
            </span>
          </div>
          <button class="s-close" @click="closeGallery">×</button>
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
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
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
          <button class="s-close" @click="closeNotifyModal">×</button>
        </div>
        <div class="s-modal-body">
          <label class="s-field-label">通知内容</label>
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

          <label class="s-field-label">宠物动作</label>
          <select v-model="notifyAction" class="s-modal-select">
            <option v-for="a in NOTIFY_ACTIONS" :key="a.value" :value="a.value">{{ a.label }}</option>
          </select>

          <div class="s-modal-tips">
            <div class="s-tips-title">调用教程</div>
            <div class="s-tips-sub">① 前端 / Tauri 命令（本应用内）</div>
            <pre class="s-code">import { pushNotify } from '@/store/notify'
pushNotify('摸鱼一下~', 'wave')  // 动作可选：wave / jump / failed / working / waiting / look / run</pre>
            <div class="s-tips-sub">② HTTP 接口（任意外部程序，端口 8756）</div>
            <pre class="s-code">curl -X POST http://127.0.0.1:8756/notify \
  -H 'Content-Type: application/json' \
  -d '{"text":"下班啦！","action":"jump"}'</pre>
            <div class="s-tips-note">提示：宠物需处于「显示」状态才能看到气泡；双击气泡或最多显示 3 条后会自动消失。</div>
          </div>
        </div>
        <div class="s-modal-foot">
          <button class="s-modal-cancel" @click="closeNotifyModal">取消</button>
          <button class="s-modal-send" :disabled="!notifyText.trim()" @click="sendNotify">发送</button>
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
  padding: 16px 20px 20px;
  /* 无边框透明窗口：CSS 自绘圆角 */
  border-radius: 14px;
  overflow: hidden;
  /* 禁止选中文字（避免拖动缩放滑块时误选中文本） */
  user-select: none;
  -webkit-user-select: none;
}
.s-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
  flex-shrink: 0;
  user-select: none;
  -webkit-user-select: none;
}
.s-title {
  font-size: 16px;
  font-weight: 700;
  color: var(--text);
}
.s-close {
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  font-size: 20px;
  line-height: 1;
  color: var(--muted);
  display: flex;
  align-items: center;
  justify-content: center;
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
  transition: filter 0.18s ease, transform 0.12s ease;
}
.s-test-btn:hover {
  filter: brightness(1.06);
}
.s-test-btn:active {
  transform: scale(0.97);
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
}
.s-modal-mask {
  position: absolute;
  inset: 0;
  background: rgba(15, 23, 42, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 60;
}
.s-modal {
  width: 92%;
  max-width: 460px;
  max-height: 90%;
  display: flex;
  flex-direction: column;
  background: var(--panel, #fff);
  border-radius: var(--radius-lg, 16px);
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
  color: var(--muted);
  margin: 4px 0 6px;
}
.s-modal-input {
  width: 100%;
  padding: 9px 11px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: var(--radius-sm, 8px);
  font-size: 13px;
  font-family: inherit;
  line-height: 1.5;
  outline: none;
  background: var(--panel, #fff);
  color: var(--text);
  resize: vertical;
  box-sizing: border-box;
}
.s-modal-input:focus {
  border-color: var(--primary);
}
.s-char-count {
  text-align: right;
  font-size: 11px;
  color: var(--muted);
  margin: 4px 0 12px;
}
.s-modal-select {
  width: 100%;
  padding: 8px 11px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: var(--radius-sm, 8px);
  font-size: 13px;
  outline: none;
  background: var(--panel, #fff);
  color: var(--text);
  cursor: pointer;
}
.s-modal-select:focus {
  border-color: var(--primary);
}
.s-modal-tips {
  margin-top: 16px;
  padding: 12px;
  background: var(--panel-2, rgba(241, 243, 247, 0.7));
  border-radius: var(--radius-sm, 8px);
}
.s-tips-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
  margin-bottom: 8px;
}
.s-tips-sub {
  font-size: 11px;
  color: var(--muted);
  margin: 8px 0 4px;
}
.s-code {
  margin: 0;
  padding: 8px 10px;
  background: rgba(31, 39, 51, 0.06);
  border-radius: 6px;
  font-size: 11px;
  line-height: 1.55;
  color: var(--text);
  white-space: pre-wrap;
  word-break: break-all;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
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
  border-radius: var(--radius-lg, 16px);
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
  flex-direction: column;
  gap: 4px;
}
.s-gallery-title {
  font-size: 17px;
  font-weight: 700;
  color: var(--text);
}
.s-gallery-source {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.4;
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
  transition: background 0.15s ease, color 0.15s ease;
}
.s-gallery-clear:hover {
  background: rgba(31, 39, 51, 0.12);
  color: var(--muted);
}
.s-gallery-refresh {
  padding: 8px 14px;
  border: 1px solid var(--border-strong, #ddd);
  border-radius: var(--radius-sm, 8px);
  background: transparent;
  color: var(--muted);
  font-size: 13px;
  cursor: pointer;
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
