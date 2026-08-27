<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed, nextTick } from 'vue'
import { isTauri, onEvent, setNotifyInteractiveRects, setPetHitRects, applyPetHitRects, startDragging, showPetWindow, hidePetWindow, resizePetWindow, onWindowMoved } from '../tauri'
import { petStore, currentPet, openPetPicker, setPetVisible, setCurrentPet, loadPetManifest, MIN_SCALE, MAX_SCALE } from '../store/pet'
import { notifyStore, consumeNotify } from '../store/notify'
import { BUILTIN_DIALOGUES, EXTERNAL_DIALOGUES } from '../pets/dialogues'
import SpritePet from './SpritePet.vue'

// macOS 判断：macOS 由原生层（macos_pet.rs 的 NSTimer）驱动 hover/drag，
// 因为 WebView 在 App 非激活时 mouseenter/mousedown 不可靠；点击/双击仍走 DOM @click/@dblclick。
const isMac =
  typeof navigator !== 'undefined' &&
  (/Mac|iPhone|iPad|iPod/i.test(navigator.platform) || /Macintosh/i.test(navigator.userAgent))

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
// hovered 守卫：原生 pet-hover 与 DOM mouseenter 可能先后触发同一状态，避免 waiting 动作重复播放。
let hovered = false
function onPetEnter(): void {
  if (hovered) return
  hovered = true
  if (petState.value === 'talk') return
  playAction('waiting', undefined, () => {
    if (petState.value === 'waiting') petState.value = 'idle'
    scheduleRandomAction()
  })
}
function onPetLeave(): void {
  if (!hovered) return
  hovered = false
  if (petState.value === 'waiting') {
    petState.value = 'idle'
    scheduleRandomAction()
  }
}

// ── Windows 像素穿透：上报可交互矩形 ──
const petStageEl = ref<HTMLElement | null>(null)
const bubbleEl = ref<HTMLElement | null>(null)

// 记录正在播放「离场动画」的气泡矩形。
//
// 根因：v-if 变 false 的瞬间，Vue 会立刻解绑 bubbleEl 这个模板 ref
// （哪怕元素因为 <Transition> 还留在 DOM 里继续播 220ms 的淡出动画）。
// reportInteractiveRects() 原本完全依赖 bubbleEl.value 去测量矩形，
// ref 一旦变 null，气泡矩形就整个从上报列表里消失——SetWindowRgn 收到
// 的新裁剪区域里根本没有这块，于是气泡在淡出动画播放到一半时就被硬裁掉，
// 而不是跟着 opacity/scale 一起自然淡出。
//
// 修复：用 Transition 的 @leave 钩子，在气泡刚开始离场、DOM 元素还没被
// ref 系统追踪但确实还在页面上时，主动测一次它的矩形并缓存下来；
// reportInteractiveRects() 在 bubbleEl.value 为 null 时改用这个缓存值，
// 直到 @after-leave（动画真正播完）才清空，此时才真正收窄裁剪区域。
type CachedRect = { left: number; top: number; width: number; height: number }
let leavingBubbleRect: CachedRect | null = null

function onBubbleLeave(el: Element): void {
  const b = el as HTMLElement
  const r = b.getBoundingClientRect()
  // 此时 leave-to 的 transform（scale(0.98) translateY(6px)）尚未应用，
  // measure 到的是离场前最后的「完整态」矩形，不是过渡中途的缩小值。
  if (r.width > 0 && r.height > 0) {
    leavingBubbleRect = { left: r.left, top: r.top, width: r.width, height: r.height }
  }
  reportInteractiveRects()
}

function onBubbleAfterLeave(): void {
  leavingBubbleRect = null
  reportInteractiveRects()
}

function reportInteractiveRects(): void {
  if (!isTauri) return
  const rects: Array<[number, number, number, number]> = []
  const scale = petStore.scale

  // box-shadow / filter:drop-shadow 都是纯视觉效果，不参与布局、
  // 不撑大元素 border-box，getBoundingClientRect() 完全不包含它们溢出的部分。
  // 这部分溢出像素若不算进上报矩形，会被 Windows 的 SetWindowRgn 直接裁掉
  // （阴影缺角/断层，甚至连带裁到边缘箭头/轮廓）。故给矩形四周统一外扩，
  // 覆盖「偏移 + 模糊半径」的最大视觉范围，外加冗余。
  //
  // .bubble 阴影: 0 6px 18px（下方最大约 6+18=24px）与 0 2px 6px（下方约 8px），
  //   取较大值 24px 做基准；箭头额外溢出 9px 已被这圈阴影外扩覆盖，无需单独补。
  // .pet-stage 的 drop-shadow(0 4px 8px)：下方最大约 4+8=12px，同样四周外扩。
  // 四向都外扩，避免只补下方而漏掉阴影模糊在左右/上方的少量扩散。
  // 阴影/模糊核实际扩散范围比 CSS 理论值更大，保守放宽避免 region 把阴影硬裁掉。
  // region 大一点只是多了少量可交互区域，不会裁掉视觉；裁小了才会露馅。
  const bubbleShadowPad = 28 * scale
  const petShadowPad = 16 * scale

  // 优先用当前挂载的气泡 ref；ref 已解绑（正在离场动画中）时，
  // 退回 onBubbleLeave 缓存的最后一次完整矩形，避免动画播放期间
  // 矩形提前消失导致气泡被硬裁切。
  const liveBubble = bubbleEl.value?.getBoundingClientRect() ?? null
  const bubbleRect =
    liveBubble && liveBubble.width > 0 && liveBubble.height > 0
      ? liveBubble
      : leavingBubbleRect
        ? {
            left: leavingBubbleRect.left,
            top: leavingBubbleRect.top,
            width: leavingBubbleRect.width,
            height: leavingBubbleRect.height,
          }
        : null

  if (bubbleRect) {
    rects.push([
      bubbleRect.left - bubbleShadowPad,
      bubbleRect.top - bubbleShadowPad,
      bubbleRect.width + bubbleShadowPad * 2,
      bubbleRect.height + bubbleShadowPad * 2,
    ])
  }
  let hasPetRect = false
  const p = petStageEl.value
  if (p) {
    const r = p.getBoundingClientRect()
    if (r.width > 0 && r.height > 0) {
      hasPetRect = true
      rects.push([
        r.left - petShadowPad,
        r.top - petShadowPad,
        r.width + petShadowPad * 2,
        r.height + petShadowPad * 2,
      ])
    }
  }
  // macOS：像素穿透用（NSTimer 轮询）
  void setNotifyInteractiveRects(rects)
  // Windows：透明区域穿透用（SetWindowRgn 裁切，需显式 apply 即时生效）。
  // 关键守卫：宠物元素尚未渲染（currentPet 异步加载未完成）时，若用「只有气泡、
  // 没有宠物」的残缺矩形去 SetWindowRgn，会把整个宠物区域裁掉——表现为启动时
  // 「偶尔异常」（取决于宠物清单加载快慢的竞态）。此时跳过 apply，保持整窗
  // 可交互（不裁切），等宠物真正渲染出来后由 currentPet 的 watch 触发重新上报。
  void setPetHitRects(rects)
  if (hasPetRect) {
    void applyPetHitRects()
  }
}

// 气泡有 enter 动画（transform: translateY(10px) scale(0.96) → 最终态，0.3s），
// getBoundingClientRect 会把这个 transform 算进去。如果在动画刚开始时就上报矩形，
// 量到的是缩小/偏移的中间态，而 Windows 的 SetWindowRgn 会用这个矩形直接裁剪窗口的
// 可绘制区域——一旦动画播完、气泡长到最终大小，就会被"动画刚开始时算出的更小矩形"
// 裁掉一块，且没有人再重新上报去纠正，裁切因此是持续存在的硬边，而不是一闪而过。
//
// 修复：reportInteractiveRects() 本身仍保留、用于「尽快恢复可交互」的粗略上报；
// 真正决定裁切边界的权威上报，交给动画确实结束之后（transitionend）再做一次，
// 并加一个兜底定时器，防止某些边缘情况下 transitionend 没有触发（比如元素在动画
// 播放途中被 v-if 提前销毁、跳过了 transitionend 事件）。
//
// 注意：兜底时长必须 < 气泡淡出动画时长（.bubble-leave-active 为 0.22s = 220ms）。
// 气泡消失时节点会被 v-if 销毁，::after 箭头不会单独派发 transitionend，
// 唯一能纠正「含气泡+箭头」命中矩形的就是该兜底定时器。若兜底(340ms)大于淡出(220ms)，
// 会出现「本体先淡没、箭头因命中矩形仍包含它而晚消失 ~120ms」的错位（Windows 硬边
// SetWindowRgn 下尤其明显）。故兜底取 200ms，确保权威纠正早于淡出完成，二者同步消失。
let settleTimer: ReturnType<typeof setTimeout> | null = null

async function reportInteractiveRectsSettled(): Promise<void> {
  // 关键修复：watch 默认 flush 时机为 'pre'，回调在 Vue 把响应式变化 patch 到
  // DOM 之前就同步触发。此时 bubbleEl.value 可能还指向旧气泡节点、或新气泡尚未
  // 完成布局，立刻测量会拿到过时/错误的矩形，导致 SetWindowRgn 裁剪区域更新滞后
  // 于气泡背景的绘制——在慢机器/主线程繁忙时表现为「箭头比气泡本体晚出现」。
  // 先 await nextTick() 等 DOM 真正更新完，再测量并上报，慢设备上也稳。
  await nextTick()
  reportInteractiveRects()
  if (settleTimer) clearTimeout(settleTimer)
  settleTimer = setTimeout(reportInteractiveRects, 200)
}

function onBubbleTransitionEnd(e: TransitionEvent): void {
  // 只处理气泡元素自身触发的 transitionend，避免子元素（比如 .bubble-scroll
  // 相关的样式变化）冒泡上来导致重复触发
  if (e.target !== e.currentTarget) return
  reportInteractiveRects()
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
// Windows 系统拖拽兜底：startDragging 后 OS 接管，mouseup 可能被吞导致 dragging 卡 true。
// 改用窗口 Moved 事件 debounce 兜底（窗口停止移动 ~180ms 即认为拖拽结束），即使
// mouseup 收不到也能恢复 dragging=false，避免下一次单击被 if(dragMoved) return 误吞。
let dragMovedTimer: ReturnType<typeof setTimeout> | null = null
let unlistenWindowMoved: (() => void) | null = null

function onPetMouseDown(e: MouseEvent): void {
  // macOS：拖拽移动由原生层（NSTimer + setFrameOrigin）全权驱动，点击/双击由
  // @click/@dblclick 处理。这里不注册 mousemove/mouseup 监听、也不调用 startDragging，
  // 避免与原生拖拽抢移动造成抖动。
  if (isMac) return
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
    // 注册窗口 Moved 兜底：系统拖拽期间窗口会高频触发 Moved，停手后约 180ms 不再移动
    // 即判定拖拽结束（Windows 上 mouseup 可能被 OS 吞掉，必须靠这个兜底恢复状态）。
    void onWindowMoved(() => {
      if (dragMovedTimer) clearTimeout(dragMovedTimer)
      dragMovedTimer = setTimeout(() => {
        dragMovedTimer = null
        if (dragging) stopDragging()
      }, 180)
    }).then((un) => {
      unlistenWindowMoved = un
    })
    void startDragging()
  }
}

// macOS 原生拖拽结束（Rust NSTimer 检测到松开左键后 emit pet-drag-end 触发）。
// 与 Windows 的 stopDragging 独立：macOS 不走前端 mousemove 监听，方向由 pet-drag 事件驱动。
function onNativeDragEnd(): void {
  if (!dragging) return
  dragging = false
  // 拖拽松手后会紧跟一个 click 事件，需在其到达前保持 dragMoved=true 以忽略它；
  // 留 80ms 延迟再复位，避免拖拽松手被误判成单击触发随机动作。
  setTimeout(() => {
    dragMoved = false
  }, 80)
  if (petState.value === 'runningLeft' || petState.value === 'runningRight') {
    petState.value = 'idle'
    scheduleRandomAction()
  }
}

function stopDragging(): void {
  if (!dragging) return
  dragging = false
  const wasDirLocked = dragDirLocked
  dragDirLocked = false
  window.removeEventListener('mousemove', onPetDragMove)
  window.removeEventListener('mouseup', onGlobalMouseUp)
  // 清理 Windows 系统拖拽兜底监听与 timer（避免 mouseup 被吞时的残留触发）
  if (dragMovedTimer) {
    clearTimeout(dragMovedTimer)
    dragMovedTimer = null
  }
  if (unlistenWindowMoved) {
    unlistenWindowMoved()
    unlistenWindowMoved = null
  }
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
//
// 修复：原来这里是 setTimeout(reportInteractiveRects, 0)，正好落在气泡入场动画
// 刚起步的那一帧，测到的是动画中间态矩形。改用 reportInteractiveRectsSettled，
// 立即做一次粗略上报保交互，动画真正结束后（transitionend 或 340ms 兜底）
// 再做一次权威上报去纠正。
watch(
  () => [currentNotify.value?.id, chatText.value, petStore.scale, petStore.visible, currentPet.value?.id],
  () => {
    void reportInteractiveRectsSettled()
  },
)

// 显隐开关联动整个窗口（隐藏宠物 = 隐藏窗口）
watch(
  () => petStore.visible,
  (v) => {
    if (!isTauri) return
    if (v) {
      void showPetWindow()
      // Windows：窗口从 hide 恢复后 SetWindowRgn 的 region 可能失效，
      // 需重新把当前命中矩形应用到窗口，否则会露出系统窗口边框/矩形轮廓。
      void applyPetHitRects()
    } else {
      void hidePetWindow()
    }
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
      // 用 setCurrentPet 而非直接改 currentId：它会同时持久化到 localStorage（重启不丢）
      // 并广播 pet-switch 到 settings 窗口（设置界面实时同步选中），否则托盘切换只在
      // main 窗口生效、重启后被 settings 的旧值覆盖。
      if (petStore.pets.some((p) => p.id === pureId)) setCurrentPet(pureId)
    })
    // 设置窗口同步缩放（settings 窗口改缩放 → 实时生效）
    onEvent('pet-scale', (payload) => {
      const s = Number(payload)
      if (Number.isFinite(s) && s >= MIN_SCALE && s <= MAX_SCALE) {
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
    // macOS：原生层 hover / drag 桥接（WebView 在 App 非激活时 mouseenter/mousedown 不可靠，
    // 由 macos_pet.rs 的 NSTimer 命中检测后 emit 事件，前端只负责播放动作/清理状态）。
    onEvent('pet-hover', (payload) => {
      const over = Boolean(payload)
      if (over) onPetEnter()
      else onPetLeave()
    })
    onEvent('pet-drag-start', () => {
      dragging = true
      dragMoved = true // 拖拽后紧跟的 click 应被忽略
    })
    onEvent('pet-drag', (payload) => {
      const dir = payload === 'left' ? 'runningLeft' : 'runningRight'
      if (currentPet.value?.actions?.[dir] && petState.value !== dir) {
        petState.value = dir
      }
    })
    onEvent('pet-drag-end', () => {
      onNativeDragEnd()
    })
  }

  scheduleRandomAction()
  // 尽早上报可交互矩形：macOS 穿透用 NSTimer 每 50ms 轮询一次，若矩形未及时上报，
  // 启动瞬间会被误判「鼠标不在宠物上」→ ignoresMouseEvents=true → 穿透。
  // reportInteractiveRectsSettled 内部会立即上报一次、并在 340ms 后再校正一次，
  // 足以覆盖首次挂载时的初始渲染 + 首次动画的时间窗口，不需要再手动加多档重试。
  await nextTick()
  void reportInteractiveRectsSettled()
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
  if (settleTimer) clearTimeout(settleTimer)
  if (dragMovedTimer) clearTimeout(dragMovedTimer)
  if (unlistenWindowMoved) unlistenWindowMoved()
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
    <Transition name="bubble" mode="out-in" @leave="onBubbleLeave" @after-leave="onBubbleAfterLeave">
      <div
        v-if="currentNotify"
        :key="currentNotify.id"
        ref="bubbleEl"
        class="bubble"
        @mousedown="onPetMouseDown"
        @transitionend="onBubbleTransitionEnd"
      >
        <div class="bubble-scroll"><span class="bubble-text">{{ currentNotify.text }}</span></div>
      </div>
    </Transition>
    <Transition name="bubble" mode="out-in" @leave="onBubbleLeave" @after-leave="onBubbleAfterLeave">
      <div
        v-if="chatText && !currentNotify"
        ref="bubbleEl"
        class="bubble chat-bubble"
        @transitionend="onBubbleTransitionEnd"
      >
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
  /* 气泡横向偏移变量：通知气泡默认 0（无偏移），搭话气泡 .chat-bubble 覆写为 -20px。
     抽成变量是为了让 enter/leave 动画在保留该偏移的同时叠加 translateY/scale，
     避免 .bubble-enter-from 的 transform 整体覆盖掉 translateX 导致入场横向跳变。 */
  --bubble-x: 0px;
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
  /* 气泡固定 14px 圆角（设置窗已独立改为 8px，气泡保持原值不跟随 --radius-window） */
  border-radius: 14px;
  box-shadow:
    0 6px 18px rgba(15, 23, 42, 0.16),
    0 2px 6px rgba(15, 23, 42, 0.1);
  /* 文字垂直居中（跨 webview 一致，修复 Windows WebView2 下通知气泡文字偏上） */
  display: flex;
  flex-direction: column;
  justify-content: center;
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
  /* 文字垂直居中（配合 .bubble 的 flex，保证单行/多行都居中，不偏上） */
  display: flex;
  flex-direction: column;
  justify-content: center;
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
  /* 完全继承 .bubble 的 padding/background/border/border-radius/box-shadow/font-size，
     保证搭话气泡与通知气泡视觉完全统一（WebView2 下两种气泡尺寸/字号/圆角一致）。
     仅保留位置偏移 transform（整体往左移 20px，让箭头对齐宠物中心偏左的嘴部）。
     偏移抽成 --bubble-x 变量：enter/leave 动画需在此基础上叠加 translateY/scale，
     否则 .bubble-enter-from 的 transform 会整体覆盖掉 translateX，动画结束瞬间横向跳变。 */
  pointer-events: auto;
  position: relative;
  width: fit-content;
  margin-left: auto;
  margin-right: auto;
  --bubble-x: calc(-20px * var(--pet-scale));
  transform: translateX(var(--bubble-x));
  max-width: calc(300px * var(--pet-scale));
}

.bubble-enter-active {
  transition: all 0.3s cubic-bezier(0.22, 1, 0.36, 1);
}
.bubble-leave-active {
  transition: all 0.22s cubic-bezier(0.55, 0, 1, 0.45);
}
.bubble-enter-from {
  /* 保留 --bubble-x 横向偏移，再叠加入场位移/缩放，避免覆盖丢失导致跳变 */
  transform: translateX(var(--bubble-x)) translateY(10px) scale(0.96);
  opacity: 0;
}
.bubble-leave-to {
  transform: translateX(var(--bubble-x)) translateY(6px) scale(0.98);
  opacity: 0;
}

</style>