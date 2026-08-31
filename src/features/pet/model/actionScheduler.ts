// 闲时动作调度纯逻辑（从 PetHost.vue 抽出，★ 不含定时器/ref，可单测）。
//
// 抽纯动机：随机动作选择 / 时长 / 间隔三处纯计算曾散落在 PetHost 的定时器回调里，
// 改动极易回归。这里只保留纯函数，状态与定时器由 Phase 7 的 useActionScheduler 持有。

import type { FrameSeq } from './types'

/** 随机闲时动作白名单（排除方向性动作）。 */
export const RANDOM_POOL = ['wave', 'jump', 'failed', 'waiting', 'working', 'look']

/** 动作播放时长(ms) = count / fps * 1000；fps 缺失回退 8。 */
export function actionDurationMs(seq: FrameSeq): number {
  return (seq.count / (seq.fps || 8)) * 1000
}

/** 从白名单里按当前可用动作随机选一个；talk 状态 / 无可用动作返回 null。 */
export function pickRandomAction(
  pool: string[],
  actions: Record<string, FrameSeq>,
  state: string,
  rng: () => number = Math.random,
): string | null {
  if (state === 'talk') return null
  const names = pool.filter((n) => actions[n])
  if (names.length === 0) return null
  return names[Math.floor(rng() * names.length)]
}

/** 下次随机动作的间隔(ms)：6~15s。 */
export function nextRandomDelayMs(rng: () => number = Math.random): number {
  return 6000 + rng() * 9000
}
