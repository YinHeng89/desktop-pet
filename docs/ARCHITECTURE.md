# PetBuddy 架构说明

> 本文档描述**当前**架构（重构开始前），并记录目标架构的演进方向。
> 完整改造清单见 [refactor/REFACTOR_PLAN.md](./refactor/REFACTOR_PLAN.md)，
> 测试方案见 [refactor/TEST_PLAN.md](./refactor/TEST_PLAN.md)。

---

## 1. 系统概览

PetBuddy 是一个 **Tauri 2 + Vue 3** 的跨平台桌面宠物 + 通用通知接收器。
一只精灵图动画宠物常驻屏幕右下角（透明无边框置顶窗口），可作为外部应用的「通知播报员」。

```
┌──────────────────────────────────────────────────────────────┐
│  外部应用 / 脚本（curl、Python、Node、worktrack …）             │
└────────────────────────┬─────────────────────────────────────┘
                         │ POST http://127.0.0.1:8756/notify
                         ▼
┌──────────────────────────────────────────────────────────────┐
│                     Tauri 主进程（Rust）                       │
│  ┌────────────┐ ┌─────────────┐ ┌────────────┐ ┌───────────┐ │
│  │notify_      │ │pet_import   │ │macos_pet / │ │autostart  │ │
│  │server       │ │(zip+webp+   │ │windows_pet │ │(SMAppSvc /│ │
│  │(手写 HTTP)  │ │ 在线画廊)   │ │(穿透)      │ │ Run 键)   │ │
│  └──────┬─────┘ └──────┬──────┘ └─────┬──────┘ └───────────┘ │
│         │              │              │                       │
│         └────── app.emit("notify-push") ──────┐               │
│                        └── invoke_handler ────┤               │
└───────────────────────────────────────────────┼───────────────┘
                         IPC (invoke / emit)    │
┌───────────────────────────────────────────────┼───────────────┐
│                  Webview（Vue 3）              │               │
│  ┌──────── main 窗口 ────────┐  ┌──── settings 窗口 ────────┐ │
│  │ PetHost.vue               │  │ PetSettings.vue           │ │
│  │  ├ SpritePet.vue          │  │  ├ SpritePet.vue（预览）  │ │
│  │  └ 气泡 / 拖拽 / 命中上报 │  │  └ 列表 / 导入 / 画廊     │ │
│  │ store/pet.ts（独立实例）  │  │ store/pet.ts（独立实例）  │ │
│  └───────────────────────────┘  └───────────────────────────┘ │
│              └── 跨窗口同步：broadcast_event → app.emit ──┘    │
└──────────────────────────────────────────────────────────────┘
```

---

## 2. 模块职责

### 2.1 Rust 侧（`src-tauri/src/`）

| 模块               | 行数 | 职责                                                                              |
| ------------------ | ---- | --------------------------------------------------------------------------------- |
| `main.rs`          | 441  | 入口：Builder 装配、窗口初始定位、托盘菜单、命令注册                              |
| `macos_pet.rs`     | 446  | macOS 像素穿透（NSTimer 16ms 轮询 `setIgnoresMouseEvents`）+ 原生 hover/drag 桥接 |
| `windows_pet.rs`   | 317  | Windows 穿透（`SetWindowRgn` 圆角矩形裁切 + Per-Monitor DPI）                     |
| `pet_import.rs`    | 716  | 外部宠物导入（zip 解压、webp 头解析、base64、越界修正）+ 在线画廊                 |
| `notify_server.rs` | 178  | 本地 HTTP 服务（`TcpListener` 手写，仅解析 `POST /notify`）                       |
| `autostart.rs`     | 128  | 开机自启（macOS `SMAppService` / Windows HKCU Run 键）                            |
| `geometry.rs`      | 91   | 跨平台纯几何（`clamp_scale` / `point_in_rects` / 矩形换算）**+ 唯一有单测的模块** |
| `interactive.rs`   | 19   | 跨平台统一命令 `update_interactive_rects`，按 cfg 分派到平台实现                  |

### 2.2 前端侧（`src/`）

| 模块                         | 行数 | 职责                                                             |
| ---------------------------- | ---- | ---------------------------------------------------------------- |
| `App.vue`                    | 21   | 按 `currentWindowLabel()` 路由到 PetHost / PetSettings           |
| `components/PetHost.vue`     | 763  | 宠物宿主：通知队列 + 动作调度 + 拖拽 + 命中矩形上报 + 跨窗口事件 |
| `components/PetSettings.vue` | 1635 | 设置窗口：列表 / 缩放 / 显隐 / 导入 / 编辑 / 画廊 / 测试通知     |
| `components/SpritePet.vue`   | 199  | 精灵图播放器（canvas 逐帧绘制）                                  |
| `store/pet.ts`               | 284  | 宠物状态 + 持久化 + 跨窗口广播                                   |
| `store/notify.ts`            | 53   | 本地通知 pending 队列                                            |
| `tauri.ts`                   | 285  | Tauri IPC 封装层（**架构上正确：单一出口**）                     |
| `pets/`                      | —    | 内置宠物清单（manifest.json）+ 台词（dialogues.ts）              |

---

## 3. 关键设计

### 3.1 双窗口 + 独立 store

两个 `WebviewWindow`（`main` / `settings`）各自加载同一个 Vue bundle，
由 `App.vue` 根据 `__TAURI_INTERNALS__.metadata.currentWindow.label` 同步路由。

**每个窗口持有独立的 `petStore` 实例**。跨窗口同步链路：

```
窗口 A 改状态
  → setCurrentPet / setPetScale / setPetVisible
  → emitEvent(...)  →  invoke("broadcast_event")
  → Rust app.emit(event, payload)   ← 广播给【所有窗口，含发起者 A 自己】
  → 窗口 A / B 的 onEvent 回调各自处理
```

**防回环机制**：`setCurrentPet` 中 `if (petStore.currentId === id) return`。
即「值已相等则不再广播」——把幂等性当成了防环手段。

> ⚠️ **已知脆弱点**（架构债 #1）：一旦将来需要「强制刷新 / 重新同步」，
> 这个 early return 会把请求吞掉；绕过它又会无限广播。
> 目标方案：事件携带 `source: windowId`，接收端跳过自己发出的事件（Phase 8）。

### 3.2 跨平台穿透

两种完全不同的机制，由前端统一入口 `update_interactive_rects` 收敛：

|        | macOS                                                                       | Windows                                              |
| ------ | --------------------------------------------------------------------------- | ---------------------------------------------------- |
| 机制   | NSTimer 每 16ms 轮询鼠标位置，命中矩形则 `setIgnoresMouseEvents:false`      | `SetWindowRgn` 把窗口静态裁成「宠物 + 气泡」圆角并集 |
| 时机   | 动态，随鼠标/窗口变化实时切换                                               | 静态，需前端显式 `apply_pet_hit_rects` 才生效        |
| 坐标系 | CSS 逻辑像素（左上原点，由 Rust 从屏幕左下原点换算）                        | 物理像素（`rect * GetDpiForWindow(hwnd)/96`）        |
| 共性   | 均由前端上报 `Array<[x, y, w, h]>`，命中语义复用 `geometry::point_in_rects` | 同左                                                 |

**前端上报时序有三个非平凡的补偿逻辑**（都写在 `PetHost.vue` 注释里，改动前务必先读）：

1. **气泡离场缓存**：`v-if` 变 false 的瞬间 Vue 会解绑 `bubbleEl` ref，但元素因 `<Transition>` 还在播 220ms 淡出动画。改用 `@leave` 钩子缓存矩形，否则气泡淡出一半被硬裁。
2. **入场动画补偿**：`getBoundingClientRect()` 会算进 `transform`，动画起步时量到的是缩小态。`reportInteractiveRectsSettled()` 先 `await nextTick()` 上报一次保交互，动画结束（`transitionend` 或 200ms 兜底）后再权威上报一次。
3. **阴影外扩**：`box-shadow` / `drop-shadow` 不参与布局，但会被 `SetWindowRgn` 裁掉。故矩形四周外扩 `28 * scale`（气泡）/ `16 * scale`（宠物）。

### 3.3 窗口尺寸与锚点

`main` 窗口尺寸按缩放比例动态计算（`main.rs::pet_window_size`）：

```
pet_w    = 192 * scale
pet_h    = 208 * scale
bubble_h = 156 * scale
w = max(pet_w, 320 * scale) + 24
h = bubble_h + pet_h + 16 + 24
```

- `320` 是**气泡横向空间的基线宽度**，而非宠物宽度——窗口因此远大于宠物本体，多出的部分是透明穿透区。
- 末尾 `+24` 是透明缓冲，落在左/上，不影响右下角视觉锚点。
- 缩放时读**旧**位置与尺寸 → `set_size` → 按旧右下角重算新左上角，保证右下角不漂移。

宠物本体的 CSS 定位：`.pet-host { position: fixed; right: 16px; bottom: 16px }`，
flex column + `align-items: flex-end`，即**锚在窗口右下角内缩 16px**。

### 3.4 外部宠物格式

`pet.json` + `spritesheet.webp` 打包为 zip。帧布局默认套用 Codex Pet V2 标准
（192×208、8 列、11 行），`pet.json` 可用 `idle` / `talk` / `actions` 覆盖。

导入时解析 webp 文件头（VP8 / VP8L / VP8X 三种编码）得到真实尺寸，
对越界 `row`（动作被移除）与 `count`（截断）做修正，避免画布清空导致宠物消失。
精灵图以 base64 data URL 返回前端，无需额外 asset 协议。

---

## 4. 数据流：一条外部通知的生命周期

```
1. curl -d '{"text":"下班啦","action":"wave"}'
                    ↓
2. notify_server::handle()  解析 HTTP → 校验字数(≤120)
                    ↓
3. app.emit("notify-push", payload)          ← 广播给所有窗口
                    ↓
4. PetHost.vue 的 onEvent("notify-push") → enqueueNotify(payload)
                    ↓
5. 若 action 非空 → playAction(action, onDone) 先播动作
   （否则直接显示气泡）
                    ↓
6. showNotify() → currentNotify = item; petState = 'talk'
                    ↓
7. SpritePet.vue 按 state 取帧段 → requestAnimationFrame 逐帧绘制
                    ↓
8. 4 秒（当前硬编码）后 → showNextNotify() 取下一条 / 置 null
                    ↓
9. 气泡 v-if 变 false → <Transition> 播 220ms 淡出
   → @leave 缓存矩形 → reportInteractiveRects() 收窄可交互区域
```

> ⚠️ 第 8 步：`duration` 字段从 HTTP → Rust → 前端全链路贯通，
> 但 `PetHost` 构造 `NotifyItem` 时未保存它，气泡时长恒为 4000ms。
> **属于文档承诺但未实现的功能**（P0-4），Phase 1 修复。

---

## 5. 当前架构债

| #   | 问题                                                      | 影响                                                   | 目标方案                                                         |
| --- | --------------------------------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------- |
| 1   | 跨窗口广播靠「值相等」防回环                              | 脆弱，无法支持强制同步                                 | 事件带 `source: windowId`（Phase 8）                             |
| 2   | 持久化分裂：宠物配置走 localStorage，窗口位置走 Rust 文件 | 两套口径；localStorage 跨 webview 是否共享未经平台验证 | 统一到 Rust `state_store`（Phase 8）                             |
| 3   | 核心常量在 4 处重复（帧尺寸、缩放范围、窗口基线宽）       | 改一处要同步四处，无机制保障                           | Rust 生成 + 契约测试校验（Phase 9.5）                            |
| 4   | 平台判断两套：Rust `#[cfg]` / 前端 UA 嗅探                | 可能不一致；`navigator.platform` 已废弃                | 前端统一用 Rust 提供的 `get_platform`（Phase 5）                 |
| 5   | `PetHost.vue` 763 行 / `PetSettings.vue` 1635 行          | 单文件职责过多，回归困难                               | 拆分为 `windows/` + `composables/` + `components/`（Phase 7）    |
| 6   | 可测试代码（纯逻辑）与 IO / 平台代码混在一起              | 全项目仅 3 个单测                                      | 抽出 `domain/`（Rust）与 `model/`（前端）纯逻辑层（Phase 2 / 6） |

---

## 6. 目标架构（重构后）

```
Rust:  main.rs(装配) → commands/(薄) → domain/(纯逻辑·可单测)
                                     → platform/(trait+cfg 实现)
                                     → infra/(IO 适配·可注入 mock)
                                     → state/(集中状态)

前端:  windows/(编排) → features/{pet,notify,gallery,import,settings}/
                          ├ model/      (纯函数·可单测)
                          ├ store/      (响应式状态)
                          ├ api/        (IPC 调用)
                          └ components/ (UI)
                      → shared/{config,platform,ipc,errors,utils,styles}
```

**依赖规则**（Phase 5 起由 ESLint `import/no-restricted-paths` 强制）：

- `model/` 仅可依赖 `shared/config`、`shared/utils`、`shared/errors` → 保证 100% 可单测
- `shared/` 不得反向依赖 `features/` 或 `windows/`
- 业务层禁止直接 `invoke`，必须走 `shared/ipc`
- `windows/` 只做编排，不含业务逻辑

---

## 7. 本地开发

```bash
npm ci                        # 安装依赖
npm run verify                # 全量验证：typecheck + lint + format + test + cargo test
bash ./scripts/tauri-dev.sh   # 启动 Tauri 桌面版
bash ./scripts/dev.sh         # 仅前端 dev（浏览器，无 Tauri 窗口/托盘/穿透）
```

单项命令：

```bash
npm run typecheck      # vue-tsc --noEmit
npm run lint           # eslint（仅 error 失败）
npm run lint:strict    # eslint --max-warnings 0
npm run format         # prettier --write + cargo fmt
npm run test           # vitest run
npm run test:cov       # vitest + 覆盖率报告
npm run test:rust      # cargo test
```

> Node 版本要求 `>= 20.19`（ESLint 10 的约束）。
> `scripts/_env.sh` 目前固定切到 Homebrew `node@20`，若其版本低于 20.19 需同步调整。

---

## 8. 变更记录

| 日期       | 变更                                                                                                                                    |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-08-31 | 引入 rustfmt / clippy / ESLint 9 / Prettier / Vitest，建立 `quality.yml` 门禁；首次全量格式化（已登记进 `.git-blame-ignore-revs` 流程） |
