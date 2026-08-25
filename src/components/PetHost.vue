<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed } from 'vue'
import { isTauri, onEvent, emitEvent, setNotifyInteractiveRects, startDragging, showPetWindow, hidePetWindow, resizePetWindow } from '../tauri'
import { petStore, currentPet, openPetPicker, setPetVisible, loadPetManifest } from '../store/pet'
import { notifyStore, consumeNotify } from '../store/notify'
import { BUILTIN_DIALOGUES, EXTERNAL_DIALOGUES } from '../pets/dialogues'
import SpritePet from './SpritePet.vue'

// ── 通知气泡（接收外部通知）──
interface NotifyItem {
  id: number
  text: string
  action?: string
}
const currentNotify = ref<NotifyItem | null>(null)
const notifyQueue: Array<NotifyItem> = []
let notifySeq = 0
let notifyTimer: ReturnType<typeof setTimeout> | null = null

// ── 单击搭话气泡（独立于通知，3 秒消失）──
const chatText = ref('')
let chatTimer: ReturnType<typeof setTimeout> | null = null

// ── 动作定时器（勿共用，否则互相 clearTimeout 导致随机动作被误清）──
let actionTimer: ReturnType<typeof setTimeout> | null = null
let randomTimer: ReturnType<typeof setTimeout> | null = null
let runTimer: ReturnType<typeof setTimeout> | null = null

// 宠物动作状态
const petState = ref<string>('idle')
const usePet = computed(() => !!currentPet.value && petStore.visible)

// 宠物真实渲染宽度（帧宽 × 缩放），用于锁定气泡容器宽度、稳定尾巴对齐。
const petWidth = computed(() => (petStore.frame?.width || 192) * petStore.scale)
const petWidthCss = computed(() => `${petWidth.value}px`)
// 尾巴对准宠物水平中心：距气泡右边缘 = 宠物半宽。
const tailRightCss = computed(() => `${petWidth.value / 2}px`)

// 通用：播放一个动作，播完回 idle 再回调 onDone
function playAction(name: string, durationMs?: number, onDone?: () => void): void {
  const seq = currentPet.value?.actions?.[name]
  if (!seq) {
    petState.value = 'idle'
    onDone?.()
    return
  }
  petState.value = name
  const dur = durationMs ?? (seq.count / (seq.fps || 8)) * 1000
  if (actionTimer) clearTimeout(actionTimer)
  actionTimer = setTimeout(() => {
    petState.value = 'idle'
    onDone?.()
  }, dur)
}

// 随机闲时动作白名单（排除方向性动作）
const RANDOM_POOL = ['wave', 'jump', 'failed', 'waiting', 'working', 'look']

function playRandomAction(): string | null {
  const actions = currentPet.value?.actions ?? {}
  const names = RANDOM_POOL.filter((n) => actions[n])
  if (names.length === 0 || petState.value === 'talk') {
    scheduleRandomAction()
    return null
  }
  const name = names[Math.floor(Math.random() * names.length)]
  playAction(name, undefined, () => scheduleRandomAction())
  return name
}

function showChat(action: string): void {
  const petId = currentPet.value?.id ?? ''
  const map = BUILTIN_DIALOGUES[petId] ?? EXTERNAL_DIALOGUES
  const lines = map[action]
  chatText.value = lines && lines.length > 0 ? lines[Math.floor(Math.random() * lines.length)] : ''
  if (chatTimer) clearTimeout(chatTimer)
  if (chatText.value) {
    chatTimer = setTimeout(() => {
      chatText.value = ''
    }, 3000)
  }
}

function scheduleRandomAction(): void {
  if (randomTimer) clearTimeout(randomTimer)
  const delay = 6000 + Math.random() * 9000
  randomTimer = setTimeout(() => {
    if (petState.value === 'idle') playRandomAction()
    else scheduleRandomAction()
  }, delay)
}

// ── 通知气泡 ──
function showNotify(item: NotifyItem): void {
  currentNotify.value = item
  petState.value = 'talk'
  const dur = 4000
  if (notifyTimer) clearTimeout(notifyTimer)
  notifyTimer = setTimeout(() => {
    petState.value = 'idle'
    showNextNotify()
  }, dur)
}

function showNextNotify(): void {
  if (notifyQueue.length === 0) {
    currentNotify.value = null
    petState.value = 'idle'
    return
  }
  const item = notifyQueue.shift()!
  showNotify(item)
}

function enqueueNotify(payload: { text?: string; action?: string; duration?: number }): void {
  const text = payload?.text ?? ''
  if (!text) return
  const item: NotifyItem = {
    id: ++notifySeq,
    text,
    action: payload?.action,
  }
  notifyQueue.push(item)
  // 若指定了动作，先播该动作（优先级最高）；动作播完后再显示气泡，
  // 否则 showNotify 会立即把 petState 切成 'talk'，覆盖动作动画导致"选了动作没生效"。
  if (item.action && item.action !== 'talk') {
    playAction(item.action, undefined, () => {
      if (!currentNotify.value) showNextNotify()
    })
  } else if (!currentNotify.value) {
    showNextNotify()
  }
}

// ── hover 宠物 ──
function onPetEnter(): void {
  if (petState.value === 'talk') return
  playAction('waiting', undefined, () => {
    if (petState.value === 'waiting') petState.value = 'idle'
    scheduleRandomAction()
  })
}
function onPetLeave(): void {
  if (petState.value === 'waiting') {
    petState.value = 'idle'
    scheduleRandomAction()
  }
}

// ── macOS 像素穿透：上报可交互矩形 ──
const petStageEl = ref<HTMLElement | null>(null)
const bubbleEl = ref<HTMLElement | null>(null)

function reportInteractiveRects(): void {
  if (!isTauri) return
  const rects: Array<[number, number, number, number]> = []
  const b = bubbleEl.value
  if (b) {
    const r = b.getBoundingClientRect()
    rects.push([r.left, r.top, r.width, r.height])
  }
  const p = petStageEl.value
  if (p) {
    const r = p.getBoundingClientRect()
    rects.push([r.left, r.top, r.width, r.height])
  }
  void setNotifyInteractiveRects(rects)
}

// ── 宠物交互：拖动跑步 + 单击随机 + 双击设置 ──
function onPetMouseDown(e: MouseEvent): void {
  const el = petStageEl.value
  if (el && petState.value !== 'talk') {
    const rect = el.getBoundingClientRect()
    const centerX = rect.left + rect.width / 2
    const dir = e.clientX < centerX ? 'runningLeft' : 'runningRight'
    if (currentPet.value?.actions?.[dir]) {
      if (runTimer) clearTimeout(runTimer)
      runTimer = setTimeout(() => {
        if (petState.value !== 'talk') petState.value = dir
      }, 120)
    }
  }
  void startDragging()
}

function onPetClick(): void {
  if (runTimer) {
    clearTimeout(runTimer)
    runTimer = null
  }
  if (petState.value !== 'talk') {
    const action = playRandomAction()
    if (action) showChat(action)
  }
}

function onGlobalMouseUp(): void {
  if (runTimer) {
    clearTimeout(runTimer)
    runTimer = null
  }
  if (petState.value === 'runningLeft' || petState.value === 'runningRight') {
    petState.value = 'idle'
    scheduleRandomAction()
  }
}

function onPetDblClick(): void {
  openPetPicker()
}

// 气泡/缩放/显隐变化时重新上报矩形
watch(
  () => [currentNotify.value?.id, chatText.value, petStore.scale, petStore.visible],
  () => {
    setTimeout(reportInteractiveRects, 0)
  },
)

// 显隐开关联动整个窗口（隐藏宠物 = 隐藏窗口）
watch(
  () => petStore.visible,
  (v) => {
    if (!isTauri) return
    if (v) void showPetWindow()
    else void hidePetWindow()
  },
)

onMounted(async () => {
  // 接收外部通知（Rust HTTP 服务 → notify-push 事件）
  if (isTauri) {
    onEvent('notify-push', (payload) => enqueueNotify(payload as { text?: string; action?: string; duration?: number }))
    // 托盘菜单切换宠物 / 打开设置
    onEvent('pet-switch', async (payload) => {
      const id = String(payload)
      if (id === 'pet:more') {
        openPetPicker()
        return
      }
      const pureId = id.replace(/^pet:/, '')
      // main 窗口的 petStore.pets 是独立实例：settings 窗口新导入的外部宠物尚未同步到这里。
      // 若本地找不到该 id，先重载外部宠物列表（含新导入项），再切换。
      if (!petStore.pets.some((p) => p.id === pureId)) {
        await loadPetManifest()
      }
      if (petStore.pets.some((p) => p.id === pureId)) petStore.currentId = pureId
    })
    // 设置窗口同步缩放（settings 窗口改缩放 → 实时生效）
    onEvent('pet-scale', (payload) => {
      const s = Number(payload)
      if (Number.isFinite(s) && s >= 0.8 && s <= 1.3) {
        petStore.scale = s
        // 窗口跟随缩放：气泡+宠物一起放大，需同步调整窗口尺寸
        void resizePetWindow(s)
      }
    })
    // 设置窗口同步显示/隐藏（settings 窗口切换「显示宠物」→ 实时生效）
    onEvent('pet-visible', (payload) => {
      petStore.visible = payload !== 'false' && payload !== false
    })
    // 托盘：显示/隐藏宠物
    onEvent('pet-toggle-visible', () => {
      setPetVisible(!petStore.visible)
    })
    // 设置窗口打开时主动请求当前选中的宠物，避免其首屏闪现第一个宠物
    onEvent('request-current-pet', () => {
      if (petStore.currentId) {
        void emitEvent('pet-switch', petStore.currentId)
      }
    })
  }

  scheduleRandomAction()
  setTimeout(reportInteractiveRects, 100)
  window.addEventListener('mouseup', onGlobalMouseUp)
  // 禁用右键菜单（透明无边框窗口，避免弹出 webview 默认菜单）
  document.addEventListener('contextmenu', (e) => e.preventDefault())

  // 启动时按当前缩放设一次窗口尺寸（窗口跟随缩放）
  if (isTauri) void resizePetWindow(petStore.scale)

  // 消费本地通知（测试通知/导入提示）
  watch(
    () => notifyStore.pending,
    () => {
      const p = consumeNotify()
      if (p) enqueueNotify(p)
    },
  )
})

onUnmounted(() => {
  if (notifyTimer) clearTimeout(notifyTimer)
  if (actionTimer) clearTimeout(actionTimer)
  if (randomTimer) clearTimeout(randomTimer)
  if (runTimer) clearTimeout(runTimer)
  if (chatTimer) clearTimeout(chatTimer)
  window.removeEventListener('mouseup', onGlobalMouseUp)
})
</script>

<template>
  <div
    v-if="usePet"
    class="pet-host"
    :style="{ '--pet-scale': petStore.scale, '--pet-w': petWidthCss, '--tail-right': tailRightCss }"
  >
    <Transition name="bubble" mode="out-in">
      <div
        v-if="currentNotify"
        :key="currentNotify.id"
        ref="bubbleEl"
        class="bubble"
        @mousedown="onPetMouseDown"
      >
        <span class="bubble-text">{{ currentNotify.text }}</span>
      </div>
    </Transition>
    <Transition name="bubble" mode="out-in">
      <div v-if="chatText && !currentNotify" class="bubble chat-bubble">
        <span class="bubble-text">{{ chatText }}</span>
      </div>
    </Transition>
    <div
      ref="petStageEl"
      class="pet-stage"
      @mouseenter="onPetEnter"
      @mouseleave="onPetLeave"
      @mousedown="onPetMouseDown"
      @click="onPetClick"
      @dblclick="onPetDblClick"
    >
      <SpritePet :state="petState" :scale="petStore.scale" />
    </div>
  </div>
</template>

<style scoped>
.pet-host {
  position: fixed;
  right: 16px;
  bottom: 16px;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  pointer-events: none;
  --pet-scale: 1;
  /* 容器宽度锁定为宠物真实宽度，气泡不再撑大容器，左右贴边距离恒定 */
  width: var(--pet-w, 192px);
  /* 禁止选中文字（气泡文字不可选中） */
  user-select: none;
  -webkit-user-select: none;
}
.pet-stage {
  pointer-events: auto;
  filter: drop-shadow(0 4px 8px rgba(15, 23, 42, 0.18));
}
.bubble {
  pointer-events: auto;
  position: relative;
  /* 宽度随文字自适应（不撑满、无最小限制），居中，最大 300px；
     与搭话气泡统一：短文字就短，长文字最多 300px，下箭头始终对准宠物中心 */
  width: fit-content;
  margin-left: auto;
  margin-right: auto;
  max-width: calc(300px * var(--pet-scale));
  margin-bottom: calc(6px * var(--pet-scale));
  padding: calc(10px * var(--pet-scale)) calc(14px * var(--pet-scale));
  background: #fff;
  border: 0.5px solid rgba(15, 23, 42, 0.06);
  border-radius: calc(14px * var(--pet-scale));
  box-shadow:
    0 6px 18px rgba(15, 23, 42, 0.16),
    0 2px 6px rgba(15, 23, 42, 0.1);
}
.bubble::after {
  content: '';
  position: absolute;
  /* 气泡已居中，下箭头用 left:50% 落在气泡底部正中，即对准宠物中心 */
  left: 50%;
  transform: translateX(-50%);
  bottom: calc(-9px * var(--pet-scale));
  width: 0;
  height: 0;
  border-left: calc(8px * var(--pet-scale)) solid transparent;
  border-right: calc(8px * var(--pet-scale)) solid transparent;
  border-top: calc(9px * var(--pet-scale)) solid #fff;
}
.bubble-text {
  font-size: calc(13px * var(--pet-scale));
  font-weight: 500;
  color: var(--text);
  line-height: 1.45;
  letter-spacing: 0.01em;
  word-break: break-word;
  white-space: pre-wrap;
}
.chat-bubble {
  /* 宽度随文字自适应，整体往左移 20px（transform 同时移动气泡与下箭头），下箭头随气泡一起左移 */
  pointer-events: auto;
  position: relative;
  width: fit-content;
  margin-left: auto;
  margin-right: auto;
  transform: translateX(calc(-20px * var(--pet-scale)));
  max-width: calc(300px * var(--pet-scale));
  margin-bottom: calc(6px * var(--pet-scale));
  padding: calc(7px * var(--pet-scale)) calc(12px * var(--pet-scale));
  background: #fff;
  border: 0.5px solid rgba(15, 23, 42, 0.06);
  border-radius: calc(14px * var(--pet-scale));
  box-shadow:
    0 6px 18px rgba(15, 23, 42, 0.16),
    0 2px 6px rgba(15, 23, 42, 0.1);
}
.chat-bubble::after {
  content: '';
  position: absolute;
  /* 气泡已水平居中，下箭头用 left:50% 落在气泡底部正中，即对准宠物中心 */
  left: 50%;
  transform: translateX(-50%);
  bottom: calc(-9px * var(--pet-scale));
  width: 0;
  height: 0;
  border-left: calc(8px * var(--pet-scale)) solid transparent;
  border-right: calc(8px * var(--pet-scale)) solid transparent;
  border-top: calc(9px * var(--pet-scale)) solid #fff;
}
.chat-bubble .bubble-text {
  font-size: calc(12px * var(--pet-scale));
}

.bubble-enter-active {
  transition: all 0.3s cubic-bezier(0.22, 1, 0.36, 1);
}
.bubble-leave-active {
  transition: all 0.22s cubic-bezier(0.55, 0, 1, 0.45);
}
.bubble-enter-from {
  transform: translateY(10px) scale(0.96);
  opacity: 0;
}
.bubble-leave-to {
  transform: translateY(6px) scale(0.98);
  opacity: 0;
}
</style>
