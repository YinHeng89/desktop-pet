// 宠物单击搭话台词（迁移自 src/pets/dialogues.ts，Phase 6 抽纯 + pickDialogue）。
//
// 每个动作绑定多条候选台词，单击播放该动作时随机选一句。内置宠物（Miku/龙神丸/Seedy）
// 各自一套性格台词；外部宠物用通用一套。

export interface DialogueMap {
  // 动作名 → 候选台词数组
  [action: string]: string[]
}

/** 内置宠物台词（按 id 区分性格） */
export const BUILTIN_DIALOGUES: Record<string, DialogueMap> = {
  miku: {
    wave: ['嗨嗨~ 我是 Miku！', '今天也一起加油哦！', '听见我的歌声了吗？'],
    jump: ['啦啦啦~ 唱起来！', '好开心呀！', '世界第一的公主殿下登场！'],
    failed: ['唔…有点难唱', '别灰心，再来一次！', '这段旋律好难呀'],
    waiting: ['我在哼歌等你哦~', '要听我唱一首吗？', '……你的下一句是什么？'],
    working: ['让我来帮忙吧！', '正在努力中~', '这段代码交给我啦！'],
    look: ['嗯？怎么啦？', '想听我唱歌吗？', '看着我入迷了吗？'],
  },
  ryujinmaru: {
    wave: ['哟，来了啊！', '并肩作战吧！', '状态不错！'],
    jump: ['太好了！', '干得漂亮！', '痛快！'],
    failed: ['可恶……再来！', '别泄气，继续！', '这里有点难缠'],
    waiting: ['随时待命', '等你指令', '准备好了吗？'],
    working: ['正在处理', '交给我', '让我来解决'],
    look: ['怎么了？', '有情况？', '我在听'],
  },
  Seedy: {
    wave: ['嗨~ 是我呀！', '你好呀！', '今天也在呢~'],
    jump: ['哇！好开心！', '耶！', '蹦蹦跳跳~'],
    failed: ['呜呜…', '别急别急', '再试一次好不好？'],
    waiting: ['等你哦~', '我在呢', '……嗯？'],
    working: ['让我看看~', '好好想想', '嗯……这个嘛'],
    look: ['咦？', '怎么啦？', '看着我干嘛呀'],
  },
}

/** 外部导入宠物的通用台词（性格中立） */
export const EXTERNAL_DIALOGUES: DialogueMap = {
  wave: ['嗨~', '你好呀！', '我在哦！'],
  jump: ['好耶！', '开心！'],
  failed: ['唔…', '别担心', '再试试'],
  waiting: ['等你哦', '在呢', '嗯？'],
  working: ['让我看看', '处理中…', '交给我'],
  look: ['嗯？', '怎么啦？', '有事吗'],
}

/** 通用兜底台词（动作名没有对应台词时用） */
export const FALLBACK_DIALOGUES: DialogueMap = {
  idle: ['……', '（发呆中）', '嗯……'],
}

/** 按宠物 id + 动作随机选一句台词；未知 id 回退外部通用台词，无对应台词返回空串。 */
export function pickDialogue(
  petId: string,
  action: string,
  rng: () => number = Math.random,
): string {
  const map = BUILTIN_DIALOGUES[petId] ?? EXTERNAL_DIALOGUES
  const lines = map[action]
  if (lines && lines.length > 0) return lines[Math.floor(rng() * lines.length)]
  return ''
}
