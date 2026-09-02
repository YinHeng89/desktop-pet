<script setup lang="ts">
// 精灵帧播放器：按 manifest 的帧布局逐帧绘制到 canvas。
// state 支持：
//   - 'idle'  → 播放 idle 段（常驻循环）
//   - 'talk'  → 播放 talk 段（说话/提示）
//   - 其他字符串 → 播放 manifest.actions[state] 段（随机闲时动作，如 wave/jump/waiting/look）
// 随机动作播放完由调用方（PetHost）定时切回 idle，这里只负责按 state 取对应帧段循环播放。
// scale 控制整体缩放（最小 0.5，最大 2）。
//
// 注意：每个组件实例持【独立】的 Image 对象，避免多实例（设置窗口预览 + 宠物窗口）
// 共享同一张图导致 onload 回调互相覆盖、动画错乱（表现为「乱闪」）。
import { onMounted, onBeforeUnmount, ref, watch, computed } from 'vue'
import { petStore, currentPet } from '../store/pet'
import { seqFor as frameSeqFor, frameBounds, isFrameInBounds } from '../features/pet/model/frame'

const props = withDefaults(
  defineProps<{
    state?: string // 动作状态：idle / talk / 随机动作名
    scale?: number
  }>(),
  { state: 'idle', scale: 1 },
)

const canvas = ref<HTMLCanvasElement | null>(null)
// 实例级图片对象（关键：不可模块级共享）
const img = new Image()
img.crossOrigin = 'anonymous'
let imgLoaded = false
let rafId = 0
let lastTs = 0
let frameIdx = 0
let acc = 0
// 当前正在播放的帧段标识（row:count），用于 tick 内检测动作切换
let curSeqKey = ''

const pet = computed(() => currentPet.value)
// 优先用当前宠物自己的帧几何（外部宠物尺寸各异），回退到全局默认 frame。
// 之前这里只取全局 petStore.frame（写死 192×208/8 列），导致非标外部包切帧错位。
const frame = computed(() => pet.value?.frame ?? petStore.frame)
const displayScale = computed(() => props.scale || petStore.scale || 1)

// 根据 state 取对应帧段：idle / talk / actions[state]。
// 纯逻辑（含 talk/idle 回退）已下沉到 features/pet/model/frame.ts，此处只做类型适配。
function seqFor(state: string) {
  return frameSeqFor(state, pet.value)
}

function drawFrame(row: number, col: number): void {
  const c = canvas.value
  const p = pet.value
  if (!c || !p) return
  const ctx = c.getContext('2d')
  if (!ctx) return
  // 图片未就绪不绘制（避免反复 drawImage 报错 / 闪白）
  if (!imgLoaded || !img.naturalWidth) return

  // ── NaN 守卫（必须放在所有越界判断之前）──
  // seq.count 为 0 时 frameIdx = (frameIdx + 1) % 0 === NaN。而 NaN 的任何大小
  // 比较都返回 false，会静默绕过下面的越界检查；一旦执行到 clearRect，随后的
  // drawImage(..., NaN, ...) 抛错并被 catch 吞掉 → 画布空白、宠物消失。
  if (!Number.isFinite(row) || !Number.isFinite(col)) return

  // ── 越界保护（关键）──
  // 外部宠物精灵图行数不统一（如 miku 只有 9 行，标准模板假设 11 行），
  // manifest 里的 row 可能越界。用运行时真实尺寸（naturalWidth/naturalHeight）
  // 计算实际行列数，越界时【不清空画布直接返回】，保持上一帧，避免 clearRect
  // 后 drawImage 失败导致的「宠物消失」。
  const fw = frame.value.width
  const fh = frame.value.height
  const bounds = frameBounds(img.naturalWidth, img.naturalHeight, { width: fw, height: fh })
  if (bounds.rows <= 0 || bounds.cols <= 0) return
  if (!isFrameInBounds(row, col, bounds)) {
    return // 越界帧：保持上一帧，不闪不消失
  }

  const sx = col * fw
  const sy = row * fh
  ctx.clearRect(0, 0, c.width, c.height)
  try {
    ctx.drawImage(img, sx, sy, fw, fh, 0, 0, c.width, c.height)
  } catch {
    /* 理论上到不了这里（已越界保护），兜底忽略 */
  }
}

function tick(ts: number): void {
  const p = pet.value
  if (!p || !imgLoaded) {
    rafId = requestAnimationFrame(tick)
    return
  }
  const seq = seqFor(props.state)
  if (!seq) {
    rafId = requestAnimationFrame(tick)
    return
  }
  // 若 seq 段（row+count 标识）发生变化，说明动作已切换，同步重置帧计数，
  // 避免用旧 frameIdx 在新 seq 上绘制越界/错误帧。
  const seqKey = `${seq.row}:${seq.count}`
  if (seqKey !== curSeqKey) {
    curSeqKey = seqKey
    frameIdx = 0
    acc = 0
    lastTs = ts
    drawFrame(seq.row, 0)
    rafId = requestAnimationFrame(tick)
    return
  }
  if (!lastTs) lastTs = ts
  const dt = ts - lastTs
  lastTs = ts
  acc += dt
  const interval = 1000 / (seq.fps || 8)
  if (acc >= interval) {
    acc = 0
    frameIdx = (frameIdx + 1) % seq.count
    drawFrame(seq.row, frameIdx)
  }
  rafId = requestAnimationFrame(tick)
}

function resetAndPlay(): void {
  frameIdx = 0
  acc = 0
  lastTs = 0
  const p = pet.value
  if (!p) return
  const seq = seqFor(props.state)
  if (!seq) {
    curSeqKey = ''
    return
  }
  curSeqKey = `${seq.row}:${seq.count}`
  drawFrame(seq.row, 0)
}

watch(
  () => props.state,
  () => resetAndPlay(),
  // 同步触发：动作切换时立即重置帧索引并绘制新动作第 0 帧，
  // 避免异步微任务延迟导致 tick 用「新动作 row + 旧动作 frameIdx」画出越界帧（清空 canvas 后 drawImage 失败 → 宠物闪没）。
  { flush: 'sync' },
)
// 监听 currentPet（而非 currentId）：
// 1. currentId 变化 → currentPet 随之变化 → 触发加载新宠物图
// 2. 关键：settings 窗口是独立 webview，pets 异步加载。挂载时 pets 可能尚未填充
//    （currentPet 为 null），loadImage 会跳过；等 pets 填充后 currentPet 从 null
//    变为对象，此 watch 保证补加载（此时 currentId 可能未变，故只 watch currentId 会漏掉）。
watch(currentPet, () => {
  loadImage()
})
/** 清空画布（不销毁 canvas 元素本身）。 */
function clearCanvas(): void {
  const c = canvas.value
  if (!c) return
  c.getContext('2d')?.clearRect(0, 0, c.width, c.height)
}

/**
 * 按「当前帧几何 × 缩放」同步 canvas 像素尺寸。
 *
 * 必须同时监听 frame 而不只是 displayScale：内置宠物是 192×208、外部宠物尺寸
 * 各异（如 256×256），切换宠物时 frame.value 会变。若不同步 canvas 尺寸，
 * drawFrame 会按新的 fw/fh 去精灵图上取源区域，却绘制到旧尺寸的画布上，
 * 表现为宠物被非等比拉伸或裁切。
 */
function applyCanvasSize(): void {
  const c = canvas.value
  if (!c) return
  const w = Math.round(frame.value.width * displayScale.value)
  const h = Math.round(frame.value.height * displayScale.value)
  // 幂等：给 width/height 赋值会清空画布，尺寸没变就别白清一次。
  if (c.width === w && c.height === h) return
  c.width = w
  c.height = h
}

watch([frame, displayScale], () => {
  applyCanvasSize()
  // 给 canvas.width / height 赋值会【清空画布】，必须立即重绘当前帧。
  // 否则要等到下一个动画间隔（最高 1 / fps 秒）才补上，表现为拖动缩放滑块时
  // 宠物周期性闪没。
  const seq = seqFor(props.state)
  if (seq && imgLoaded) drawFrame(seq.row, frameIdx)
})

function loadImage(): void {
  const p = pet.value
  if (!p) return
  imgLoaded = false
  // 切宠物时先清空画布：新图加载完成前（外部宠物是数 MB 的 base64 data URL，
  // 加载耗时肉眼可见）canvas 上仍留着上一只宠物的最后一帧，表现为「残影」。
  clearCanvas()
  img.onload = () => {
    imgLoaded = true
    resetAndPlay()
  }
  img.onerror = () => {
    imgLoaded = false
    console.error('[SpritePet] 精灵图加载失败:', p.spritesheet)
  }
  // 外部宠物用 base64 data URL；内置宠物走 public 静态路径（Tauri/webview 根路径均为 '/'）
  img.src = p.external ? p.spritesheet : '/pets/' + p.spritesheet
}

onMounted(() => {
  applyCanvasSize()
  loadImage()
  rafId = requestAnimationFrame(tick)
})
onBeforeUnmount(() => {
  if (rafId) cancelAnimationFrame(rafId)
})
</script>

<template>
  <canvas
    ref="canvas"
    class="sprite-pet"
    :style="{
      width: Math.round(frame.width * displayScale) + 'px',
      height: Math.round(frame.height * displayScale) + 'px',
    }"
  />
</template>

<style scoped>
.sprite-pet {
  display: block;
  /* Codex 精灵为高清插画，使用平滑缩放，避免 pixelated 产生锯齿 */
}
</style>
