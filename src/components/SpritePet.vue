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
const frame = computed(() => petStore.frame)
const displayScale = computed(() => props.scale || petStore.scale || 1)

// 根据 state 取对应帧段：idle / talk / actions[state]
function seqFor(state: string) {
  const p = pet.value
  if (!p) return null
  if (state === 'talk') return p.talk
  if (state === 'idle') return p.idle
  const a = p.actions?.[state]
  return a ?? p.idle
}

function drawFrame(row: number, col: number): void {
  const c = canvas.value
  const p = pet.value
  if (!c || !p) return
  const ctx = c.getContext('2d')
  if (!ctx) return
  // 图片未就绪不绘制（避免反复 drawImage 报错 / 闪白）
  if (!imgLoaded || !img.naturalWidth) return
  const fw = frame.value.width
  const fh = frame.value.height

  // ── 越界保护（关键）──
  // 外部宠物精灵图行数不统一（如 miku 只有 9 行，标准模板假设 11 行），
  // manifest 里的 row 可能越界。用运行时真实尺寸（naturalWidth/naturalHeight）
  // 计算实际行列数，越界时【不清空画布直接返回】，保持上一帧，避免 clearRect
  // 后 drawImage 失败导致的「宠物消失」。
  const realCols = Math.floor(img.naturalWidth / fw)
  const realRows = Math.floor(img.naturalHeight / fh)
  if (realCols <= 0 || realRows <= 0) return
  if (row >= realRows || col >= realCols) {
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
watch(displayScale, (s) => {
  const c = canvas.value
  if (c) {
    c.width = Math.round(frame.value.width * s)
    c.height = Math.round(frame.value.height * s)
  }
})

function loadImage(): void {
  const p = pet.value
  if (!p) return
  imgLoaded = false
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
  const c = canvas.value
  if (c) {
    c.width = Math.round(frame.value.width * displayScale.value)
    c.height = Math.round(frame.value.height * displayScale.value)
  }
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
