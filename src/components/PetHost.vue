<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, computed, nextTick } from 'vue'
import {
  isTauri,
  updateInteractiveRects,
  applyPetHitRects,
  startDragging,
  showPetWindow,
  hidePetWindow,
  resizePetWindow,
  onWindowMoved,
} from '../tauri'
import { useTauriEvent } from '../composables/useTauriEvent'
import {
  petStore,
  currentPet,
  openPetPicker,
  setPetVisible,
  setCurrentPet,
  loadPetManifest,
  MIN_SCALE,
  MAX_SCALE,
} from '../store/pet'
import { notifyStore, consumeNotify } from '../store/notify'
import SpritePet from './SpritePet.vue'
// ── 已抽纯的领域逻辑（★ 零 Vue 依赖，见各自文件的 spec）──
// 迁移前这些规则以字面量形式散落在本组件的定时器回调里，与 model 层形成两份
// 平行实现：spec 全绿但生产根本不走，任何一方改动都会静默分叉。此处统一接线。
import {
  normalizeDuration,
  NotifyQueue,
  shouldPlayActionFirst,
  type NotifyItem,
} from '../features/notify/model/notifyQueue'
import {
  RANDOM_POOL,
  actionDurationMs,
  pickRandomAction,
  nextRandomDelayMs,
} from '../features/pet/model/actionScheduler'
import { computeHitRects, type MeasuredRect } from '../features/pet/model/geometry'
import { pickDialogue } from '../features/pet/model/dialogues'
import { TIMING, DRAG, BUBBLE } from '../shared/config/constants'
import { isMacOS } from '../shared/platform'

// macOS 判断：macOS 由原生层（macos_pet.rs 的 NSTimer）驱动 hover/drag，
// 因为 WebView 在 App 非激活时 mouseenter/mousedown 不可靠；点击/双击仍走 DOM @click/@dblclick。
// 平台探测统一走 shared/platform（优先 Rust 结果，UA 仅作首帧降级），不在组件里嗅探 navigator。
//
// ⚠️ 必须用 computed 而非模块级常量：`get_platform` 是异步 invoke，由 App.vue 在
// onMounted 里填充缓存，而【子组件 onMounted 先于父组件执行】。若在这里一次性求值，
// 首帧必然拿到降级结果，macOS 上会错误地走 DOM 拖拽/悬停方案。
const isMac = computed(() => isMacOS())

// ── 通知气泡（接收外部通知）──
//
// 时长规则（归一化 / 上限截断）已下沉到 features/notify/model/notifyQueue.ts，
// 此处只保留渲染状态与定时器，不再维护第二份常量。
const currentNotify = ref<NotifyItem | null>(null)
// 纯 FIFO 队列（入队/出队规则可单测，与 Vue 状态解耦）。
const notifyQueue = new NotifyQueue()
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

// 通用：播放一个动作，播完回 idle 再回调 onDone
function playAction(name: string, durationMs?: number, onDone?: () => void): void {
  const seq = currentPet.value?.actions?.[name]
  if (!seq) {
    petState.value = 'idle'
    onDone?.()
    return
  }
  petState.value = name
  const dur = durationMs ?? actionDurationMs(seq)
  if (actionTimer) clearTimeout(actionTimer)
  actionTimer = setTimeout(() => {
    petState.value = 'idle'
    onDone?.()
  }, dur)
}

function playRandomAction(): string | null {
  const actions = currentPet.value?.actions ?? {}
  const name = pickRandomAction(RANDOM_POOL, actions, petState.value)
  if (!name) {
    scheduleRandomAction()
    return null
  }
  playAction(name, undefined, () => scheduleRandomAction())
  return name
}

function showChat(action: string): void {
  const petId = currentPet.value?.id ?? ''
  chatText.value = pickDialogue(petId, action)
  if (chatTimer) clearTimeout(chatTimer)
  if (chatText.value) {
    chatTimer = setTimeout(() => {
      chatText.value = ''
    }, TIMING.CHAT_MS)
  }
}

function scheduleRandomAction(): void {
  if (randomTimer) clearTimeout(randomTimer)
  const delay = nextRandomDelayMs()
  randomTimer = setTimeout(() => {
    if (petState.value === 'idle') playRandomAction()
    else scheduleRandomAction()
  }, delay)
}

// ── 通知气泡 ──
function showNotify(item: NotifyItem): void {
  currentNotify.value = item
  petState.value = 'talk'
  const dur = normalizeDuration(item.duration)
  if (notifyTimer) clearTimeout(notifyTimer)
  notifyTimer = setTimeout(() => {
    petState.value = 'idle'
    showNextNotify()
  }, dur)
}

function showNextNotify(): void {
  const item = notifyQueue.dequeue()
  if (!item) {
    currentNotify.value = null
    petState.value = 'idle'
    return
  }
  showNotify(item)
}

function enqueueNotify(payload: { text?: string; action?: string; duration?: number }): void {
  // 空文本不入队（model 层返回 null）；notifySeq 自增也一并下沉，避免两处维护。
  const item = notifyQueue.enqueue(payload)
  if (!item) return
  // 若指定了动作，先播该动作（优先级最高）；动作播完后再显示气泡，
  // 否则 showNotify 会立即把 petState 切成 'talk'，覆盖动作动画导致"选了动作没生效"。
  if (shouldPlayActionFirst(item)) {
    playAction(item.action as string, undefined, () => {
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

/** DOMRect → 纯数据矩形（model 层不依赖 DOM 类型，便于单测）。 */
function toMeasured(r: DOMRect): MeasuredRect {
  return { left: r.left, top: r.top, width: r.width, height: r.height }
}

function reportInteractiveRects(): void {
  if (!isTauri) return
  const scale = petStore.scale

  // 矩形需要按 scale 外扩一圈以覆盖 box-shadow / drop-shadow 的溢出像素
  // （阴影不参与布局、不撑大 border-box，getBoundingClientRect 完全不含它）。
  // 不扩会被 Windows 的 SetWindowRgn 直接裁掉，表现为阴影缺角/断层。
  // 外扩基准值（气泡 28 / 宠物 16，均 ×scale）见 features/pet/model/geometry.ts。

  // 优先用当前挂载的气泡 ref；ref 已解绑（正在离场动画中）时，
  // 退回 onBubbleLeave 缓存的最后一次完整矩形，避免动画播放期间
  // 矩形提前消失导致气泡被硬裁切。
  const liveBubble = bubbleEl.value?.getBoundingClientRect() ?? null
  const bubbleRect =
    liveBubble && liveBubble.width > 0 && liveBubble.height > 0 ? toMeasured(liveBubble) : null
  const bubble = bubbleRect ?? leavingBubbleRect

  // 关键守卫：宠物必须真正加载完（currentPet 就绪）才把宠物区算进可交互矩形。
  // 仅 petStageEl 有尺寸不够——容器即使宠物图未加载完也可能有布局尺寸，
  // 此时算进宠物 rect 会得到「空白/旧图」的错位置。
  const p = petStageEl.value
  let pet: MeasuredRect | null = null
  if (p && currentPet.value) {
    const r = p.getBoundingClientRect()
    if (r.width > 0 && r.height > 0) pet = toMeasured(r)
  }

  // 纯计算下沉到 model：含「宠物未就绪却只有气泡矩形 → 上报空数组保持整窗
  // 可交互」的守卫，避免 Windows 用残缺矩形把整个宠物区裁掉（启动竞态偶发点不动）。
  const { rects, hasPetRect } = computeHitRects({ bubble, pet, scale })

  // 统一上报可交互矩形（macOS/Windows 同一入口；Linux 目前 no-op）。
  // 旧命令 set_pet_hit_rects 已废弃，统一走 update_interactive_rects；
  // apply_pet_hit_rects 仅 Windows 显式生效 SetWindowRgn 时需要。
  void updateInteractiveRects(rects)
  // Windows：SetWindowRgn 裁切需显式 apply 即时生效。
  // 注意：macOS 走 NSTimer 动态穿透，不需要 applyPetHitRects，但多调用一次 harmless。
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
// 唯一能纠正「含气泡+箭头」命中矩形的就是该兜底定时器。若兜底大于淡出时长，
// 会出现「本体先淡没、箭头因命中矩形仍包含它而晚消失」的错位（Windows 硬边
// SetWindowRgn 下尤其明显）。故取 BUBBLE.SETTLE_MS = 200ms，确保权威纠正早于
// 淡出完成，二者同步消失。
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
  settleTimer = setTimeout(reportInteractiveRects, BUBBLE.SETTLE_MS)
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
// 拖拽位移阈值统一取 shared/config/constants.ts 的 DRAG.THRESHOLD_PX（6px）。
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
// 拖拽「松手后紧跟的 click」抑制窗口。
let clickGuardTimer: ReturnType<typeof setTimeout> | null = null
let unlistenWindowMoved: (() => void) | null = null

function onPetMouseDown(e: MouseEvent): void {
  // macOS：拖拽移动由原生层（NSTimer + setFrameOrigin）全权驱动，点击/双击由
  // @click/@dblclick 处理。这里不注册 mousemove/mouseup 监听、也不调用 startDragging，
  // 避免与原生拖拽抢移动造成抖动。
  if (isMac.value) return
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
  if (Math.abs(dx) < DRAG.THRESHOLD_PX && Math.abs(dy) < DRAG.THRESHOLD_PX) {
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
  //
  // 注意：这里【不再】注册 onWindowMoved。原实现每次拖拽都在异步 then 里挂一个
  // 新监听，且 `onWindowMoved(...).then(un => unlistenWindowMoved = un)` 与
  // stopDragging() 存在竞态——若拖拽先结束，unlistenWindowMoved 此刻仍是 null，
  // 取消不掉；等 promise resolve 后才把 un 赋上去，该监听从此无人取消，
  // 多次拖拽会累积多个回调。现改为在 onMounted 里注册一次，用 dragging 标志位控
  // 制是否响应（见 onWindowDragMoved）。
  if (!dragStartedOs) {
    dragStartedOs = true
    void startDragging()
  }
}

/**
 * 窗口移动兜底：系统拖拽期间窗口会高频触发 Moved，停手后约 180ms 不再移动
 * 即判定拖拽结束。Windows 上 mouseup 常被 OS 吞掉，必须靠这个兜底恢复状态，
 * 否则 dragging 卡 true，下一次单击会被 `if (dragMoved) return` 误吞。
 */
function onWindowDragMoved(): void {
  // macOS 由原生层（macos_pet.rs 的 NSTimer）全权驱动拖拽，drag-end 由
  // pet-drag-end 事件通知。若这里提前 stopDragging()，dragging 会被置 false，
  // 导致 onNativeDragEnd 的 `if (!dragging) return` 提前返回 → 跑步动作卡住不回 idle。
  if (isMac.value || !dragging) return
  if (dragMovedTimer) clearTimeout(dragMovedTimer)
  dragMovedTimer = setTimeout(() => {
    dragMovedTimer = null
    if (dragging) stopDragging()
  }, DRAG.MOVED_DEBOUNCE_MS)
}

// macOS 原生拖拽结束（Rust NSTimer 检测到松开左键后 emit pet-drag-end 触发）。
// 与 Windows 的 stopDragging 独立：macOS 不走前端 mousemove 监听，方向由 pet-drag 事件驱动。
function onNativeDragEnd(): void {
  if (!dragging) return
  dragging = false
  // 拖拽松手后会紧跟一个 click 事件，需在其到达前保持 dragMoved=true 以忽略它；
  // 留 80ms 延迟再复位，避免拖拽松手被误判成单击触发随机动作。
  // 句柄需保留：组件若在窗口期内卸载，裸 setTimeout 仍会触碰已销毁的状态。
  if (clickGuardTimer) clearTimeout(clickGuardTimer)
  clickGuardTimer = setTimeout(() => {
    clickGuardTimer = null
    dragMoved = false
  }, DRAG.CLICK_GUARD_MS)
  if (petState.value === 'runningLeft' || petState.value === 'runningRight') {
    petState.value = 'idle'
    scheduleRandomAction()
  }
}

/** 结束一次拖拽并复位状态（Windows mouseup / Moved 兜底 / macOS 原生 drag-end 共用）。 */
function stopDragging(): void {
  if (!dragging) return
  dragging = false
  const wasDirLocked = dragDirLocked
  dragDirLocked = false
  window.removeEventListener('mousemove', onPetDragMove)
  window.removeEventListener('mouseup', onGlobalMouseUp)
  // 清理 Windows 系统拖拽兜底 timer（避免 mouseup 被吞时的残留触发）。
  // onWindowMoved 监听是全局常驻的（onMounted 注册一次），不在此处取消。
  if (dragMovedTimer) {
    clearTimeout(dragMovedTimer)
    dragMovedTimer = null
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

// 消费本地通知（测试通知/导入提示）。
//
// ⚠️ 必须注册在 <script setup> 顶层，不能放进 async 的 onMounted 里。
// 原因：onMounted 的回调在 `await` 之后恢复执行时，Vue 的 currentInstance
// 已被重置为 null，此时创建的 watch 会脱离组件的 effect scope——
// 组件 unmount 时不会被停止，watcher 永久存活并持续消费 notifyStore.pending，
// 造成泄漏（HMR / 窗口重建场景下会累积出多个消费者，通知被随机抢走）。
// 注册在顶层则由组件 scope 托管，随 unmount 自动停止。
watch(
  () => notifyStore.pending,
  () => {
    const p = consumeNotify()
    if (p) enqueueNotify(p)
  },
)

// 气泡/宠物/缩放/显隐变化时重新上报矩形。
// 关键：currentPet 异步加载完成后必须重新上报，否则矩形停留在空的初始值，
// 导致命中判断失效（宠物区域无法交互）。
//
// 修复：原来这里是 setTimeout(reportInteractiveRects, 0)，正好落在气泡入场动画
// 刚起步的那一帧，测到的是动画中间态矩形。改用 reportInteractiveRectsSettled，
// 立即做一次粗略上报保交互，动画真正结束后（transitionend 或 BUBBLE.SETTLE_MS 兜底）
// 再做一次权威上报去纠正。
watch(
  () => [
    currentNotify.value?.id,
    chatText.value,
    petStore.scale,
    petStore.visible,
    currentPet.value?.id,
  ],
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

// ── Rust / 其它窗口发来的事件 ──
//
// 统一用 useTauriEvent 注册：它在组件卸载时自动取消监听。
// 迁移前这里是在 async onMounted 里直接调 onEvent(...)，9 个监听一个都没保存
// 返回值、onUnmounted 也没清理——组件销毁后监听继续存活。
//
// ⚠️ 必须写在 <script setup> 顶层：useTauriEvent 内部要注册 onUnmounted，
// 一旦越过 await，currentInstance 为 null 就无法与组件实例关联。
if (isTauri) {
  // 外部通知（Rust HTTP 服务 → notify-push 事件）
  useTauriEvent('notify-push', (payload) =>
    enqueueNotify(payload as { text?: string; action?: string; duration?: number }),
  )
  // 托盘菜单切换宠物 / 打开设置
  useTauriEvent('pet-switch', async (payload) => {
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
  useTauriEvent('pet-scale', (payload) => {
    const s = Number(payload)
    if (Number.isFinite(s) && s >= MIN_SCALE && s <= MAX_SCALE) {
      petStore.scale = s
      // 窗口跟随缩放：气泡+宠物一起放大，需同步调整窗口尺寸
      void resizePetWindow(s)
    }
  })
  // 设置窗口同步显示/隐藏（settings 窗口切换「显示宠物」→ 实时生效）
  useTauriEvent('pet-visible', (payload) => {
    petStore.visible = payload !== 'false' && payload !== false
  })
  // 托盘：显示/隐藏宠物
  useTauriEvent('pet-toggle-visible', () => {
    setPetVisible(!petStore.visible)
  })
  // macOS：原生层 hover / drag 桥接（WebView 在 App 非激活时 mouseenter/mousedown 不可靠，
  // 由 macos_pet.rs 的 NSTimer 命中检测后 emit 事件，前端只负责播放动作/清理状态）。
  useTauriEvent('pet-hover', (payload) => {
    const over = Boolean(payload)
    if (over) onPetEnter()
    else onPetLeave()
  })
  useTauriEvent('pet-drag-start', () => {
    dragging = true
    dragMoved = true // 拖拽后紧跟的 click 应被忽略
  })
  useTauriEvent('pet-drag', (payload) => {
    const dir = payload === 'left' ? 'runningLeft' : 'runningRight'
    if (currentPet.value?.actions?.[dir] && petState.value !== dir) {
      petState.value = dir
    }
  })
  useTauriEvent('pet-drag-end', () => {
    onNativeDragEnd()
  })
}

/** 透明无边框窗口下屏蔽 webview 默认右键菜单 */
function onContextMenu(e: Event): void {
  e.preventDefault()
}

onMounted(async () => {
  scheduleRandomAction()
  // Windows 系统拖拽兜底监听：整个组件生命周期【只注册一次】。
  // 必须在所有 await 之前注册：onWindowMoved 是异步的，若等下面的 nextTick /
  // resize 之后再注册，快速拖拽时监听可能尚未就绪，兜底失效。
  //
  // 这里不按平台过滤：注册时机早于 initPlatform 完成（子组件 onMounted 先于
  // 父组件），平台判断改在回调里用 computed 的 isMac 做，macOS 上回调直接 return。
  if (isTauri) {
    unlistenWindowMoved = await onWindowMoved(onWindowDragMoved)
  }
  // 尽早上报可交互矩形：macOS 穿透用 NSTimer 每 50ms 轮询一次，若矩形未及时上报，
  // 启动瞬间会被误判「鼠标不在宠物上」→ ignoresMouseEvents=true → 穿透。
  // reportInteractiveRectsSettled 内部会立即上报一次、并在 BUBBLE.SETTLE_MS
  // 后再校正一次，足以覆盖首次挂载时的初始渲染 + 首次动画的时间窗口，
  // 不需要再手动加多档重试。
  await nextTick()
  void reportInteractiveRectsSettled()
  // 禁用右键菜单（透明无边框窗口，避免弹出 webview 默认菜单）
  document.addEventListener('contextmenu', onContextMenu)

  // 启动时按当前缩放设一次窗口尺寸（窗口跟随缩放）
  if (isTauri) void resizePetWindow(petStore.scale)
})

onUnmounted(() => {
  if (notifyTimer) clearTimeout(notifyTimer)
  if (actionTimer) clearTimeout(actionTimer)
  if (randomTimer) clearTimeout(randomTimer)
  if (chatTimer) clearTimeout(chatTimer)
  if (settleTimer) clearTimeout(settleTimer)
  if (dragMovedTimer) clearTimeout(dragMovedTimer)
  if (clickGuardTimer) clearTimeout(clickGuardTimer)
  if (unlistenWindowMoved) {
    unlistenWindowMoved()
    unlistenWindowMoved = null
  }
  document.removeEventListener('contextmenu', onContextMenu)
  window.removeEventListener('mouseup', onGlobalMouseUp)
  window.removeEventListener('mousemove', onPetDragMove)
})
</script>

<template>
  <div v-if="usePet" class="pet-host" :style="{ '--pet-scale': petStore.scale }">
    <Transition
      name="bubble"
      mode="out-in"
      @leave="onBubbleLeave"
      @after-leave="onBubbleAfterLeave"
    >
      <div
        v-if="currentNotify"
        :key="currentNotify.id"
        ref="bubbleEl"
        class="bubble"
        @mousedown="onPetMouseDown"
        @transitionend="onBubbleTransitionEnd"
      >
        <div class="bubble-scroll">
          <span class="bubble-text">{{ currentNotify.text }}</span>
        </div>
      </div>
    </Transition>
    <Transition
      name="bubble"
      mode="out-in"
      @leave="onBubbleLeave"
      @after-leave="onBubbleAfterLeave"
    >
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
