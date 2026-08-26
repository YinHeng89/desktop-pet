<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed, nextTick } from 'vue'
import { isTauri, onEvent, setNotifyInteractiveRects, setPetHitRects, applyPetHitRects, startDragging, showPetWindow, hidePetWindow, resizePetWindow } from '../tauri'
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
    // 过滤零尺寸矩形（元素尚未布局时宽高为 0，会导致命中判断恒假 → 穿透）
    if (r.width > 0 && r.height > 0) {
      rects.push([r.left, r.top, r.width, r.height])
    }
  }
  const p = petStageEl.value
  if (p) {
    const r = p.getBoundingClientRect()
    if (r.width > 0 && r.height > 0) {
      rects.push([r.left, r.top, r.width, r.height])
    }
  }
  // macOS：像素穿透用（NSTimer 轮询）
  void setNotifyInteractiveRects(rects)
  // Windows：透明区域穿透用（SetWindowRgn 裁切，需显式 apply 即时生效）
  void setPetHitRects(rects)
  void applyPetHitRects()
}

// ── 宠物交互：拖动跑步 + 单击随机 + 双击设置 ──
//
// 修正说明（原实现的问题）：
// 1. 原来的方向判断用的是 mousedown 那一瞬间「鼠标落点相对宠物中心的左右」，
//    跟实际拖拽方向毫无关系——落点在左边、往右拖，也会被判成 runningLeft，方向经常是反的。
//    现在改成：持续跟踪 mousemove 的真实位移 dx，用 dx 的正负判断方向，且是「谁的绝对值
//    先达到阈值就用谁」，方向会随手指持续移动实时更新，而不是只在按下瞬间判一次。
// 2. 原来用固定 120ms 的 setTimeout 才切换到跑步动作，这个延迟和 startDragging() 触发的
//    系统原生拖拽是竞态关系：如果拖拽在 120ms 内就结束（快速拖一下就松手），setTimeout 里
//    的赋值根本来不及执行，看起来就是「拖了但没反应」。现在改成基于位移阈值触发，
//    不再依赖固定时间，只要移动够了立刻生效，跟操作快慢无关。
// 3. 阈值判断从「时间够不够」换成「位移够不够」（DRAG_DIRECTION_THRESHOLD_PX），
//    避免手抖/误触在原地被误判为拖拽。
const DRAG_DIRECTION_THRESHOLD_PX = 6

let dragging = false
let dragMoved = false // 本次按下是否真的产生了拖拽位移（用于区分单击/拖拽）
let dragStartX = 0
let dragStartY = 0
let dragDirLocked = false // 本次拖拽是否已经判定过方向（判定后不再频繁切换，避免抖动闪烁）
let dragStartedOs = false // 是否已经调用过系统级 startDragging（只调一次）

function onPetMouseDown(e: MouseEvent): void {
  // 双击兜底：Windows 上 startDragging 会吞掉 dblclick，这里用 mousedown 的 detail 直接拦截
  if (e.detail === 2) {
    openPetPicker()
    return
  }
  if (petState.value === 'talk') {
    void startDragging()
    return
  }
  dragging = true
  dragMoved = false
  dragDirLocked = false
  dragStartedOs = false
  dragStartX = e.clientX
  dragStartY = e.clientY
  window.addEventListener('mousemove', onPetDragMove)
  window.addEventListener('mouseup', onGlobalMouseUp)
  // 注意：不再在 mousedown 内同步调用 startDragging()。
  // macOS 上 mousedown 立即 startDragging 没问题，但在 Windows 上会吞掉后续
  // 的 click / dblclick / mousemove，导致「单击搭话」「双击打开设置」「左右拖动方向」
  // 全部失效。改为：真正移动超过阈值时（onPetDragMove）才调用系统拖拽，
  // 这样不移动 = 纯点击（click/dblclick 正常派发），移动 = 拖拽。
}

function onPetDragMove(e: MouseEvent): void {
  if (!dragging || petState.value === 'talk') return

  const dx = e.clientX - dragStartX
  const dy = e.clientY - dragStartY

  // 位移未达阈值：可能是手抖或即将单击，先不判定、也不启动系统拖拽
  if (Math.abs(dx) < DRAG_DIRECTION_THRESHOLD_PX && Math.abs(dy) < DRAG_DIRECTION_THRESHOLD_PX) {
    return
  }

  // 首次超过阈值：标记已移动，并按当前水平方向预判跑步动作（早于系统拖拽接管，
  // 避免 Windows 上 startDragging 之后 webview 收不到 mousemove 导致方向永远不更新）。
  if (!dragMoved) {
    dragMoved = true
    if (!dragDirLocked && Math.abs(dx) >= Math.abs(dy)) {
      const dir = dx < 0 ? 'runningLeft' : 'runningRight'
      if (currentPet.value?.actions?.[dir]) {
        dragDirLocked = true
        petState.value = dir
      }
    }
  }

  // 启动系统级窗口拖拽（只调一次）。调用后 OS 接管移动，webview 的 mousemove 在
  // Windows 上可能不再派发——但方向已经在上面预判好了，不影响跑步动作展示。
  if (!dragStartedOs) {
    dragStartedOs = true
    void startDragging()
  }
}

function stopDragging(): void {
  if (!dragging) return
  dragging = false
  const wasDirLocked = dragDirLocked
  dragDirLocked = false
  window.removeEventListener('mousemove', onPetDragMove)
  window.removeEventListener('mouseup', onGlobalMouseUp)
  if (wasDirLocked) {
    if (petState.value === 'runningLeft' || petState.value === 'runningRight') {
      petState.value = 'idle'
      scheduleRandomAction()
    }
  }
}

function onPetClick(): void {
  // 只有「没有产生方向锁定的拖拽（即没有真正移动）」才当作一次真正的单击，
  // 避免拖拽松手时被误判成单击触发随机动作（Windows 下拖拽也会派发 click）。
  if (dragMoved) return
  if (petState.value !== 'talk') {
    const action = playRandomAction()
    if (action) showChat(action)
  }
}

function onPetDblClick(): void {
  // 主路径：macOS 等 dblclick 正常派发的平台。
  // Windows 兜底已在 onPetMouseDown(e.detail===2) 处理，这里直接调用即可（重复调用无害）。
  openPetPicker()
}

function onGlobalMouseUp(): void {
  stopDragging()
}

// 气泡/宠物/缩放/显隐变化时重新上报矩形。
// 关键：currentPet 异步加载完成后必须重新上报，否则矩形停留在空的初始值，
// 导致命中判断失效（宠物区域无法交互）。
watch(
  () => [currentNotify.value?.id, chatText.value, petStore.scale, petStore.visible, currentPet.value?.id],
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
  }

  scheduleRandomAction()
  // 尽早并多次上报可交互矩形：macOS 穿透用 NSTimer 每 50ms 轮询一次，
  // 若矩形未及时上报，启动瞬间会被误判「鼠标不在宠物上」→ ignoresMouseEvents=true → 穿透。
  // 因此用 nextTick 立即上报，并用多档重试兜底 async import/invoke 的延迟。
  await nextTick()
  void reportInteractiveRects()
  setTimeout(reportInteractiveRects, 50)
  setTimeout(reportInteractiveRects, 150)
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
  if (chatTimer) clearTimeout(chatTimer)
  window.removeEventListener('mouseup', onGlobalMouseUp)
  window.removeEventListener('mousemove', onPetDragMove)
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
        <div class="bubble-scroll"><span class="bubble-text">{{ currentNotify.text }}</span></div>
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
  /* 容器最大宽度锁定为 280px（略宽于宠物真实宽度，给气泡两侧留出余量），
     气泡通过 .pet-stage(relative) 内部 absolute 定位、相对画布居中，
     不会撑大容器，左右贴边距离恒定 */
  max-width: 280px;
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
  /* 固定 14px（与设置窗统一用 --radius-window），不随宠物缩放变化，保证两端圆角一致 */
  border-radius: var(--radius-window);
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
.bubble-scroll {
  max-height: calc(120px * var(--pet-scale));
  overflow-y: auto;
  /* 隐藏滚动条视觉（仍保留滚轮/触摸滚动能力），避免单行文本因亚像素误判而显示滚动条 */
  scrollbar-width: none;
  -ms-overflow-style: none;
}
.bubble-scroll::-webkit-scrollbar {
  display: none;
}
.bubble-text {
  display: block;
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
  /* 固定 14px（与设置窗统一用 --radius-window），保证两端圆角一致 */
  border-radius: var(--radius-window);
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