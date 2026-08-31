# PetBuddy 商业化重构改造清单

> **目标**：在**完整保留现有全部功能**的前提下，把项目从"能跑的个人项目"重构成"可长期演进的商业软件"，
> 并最终建立**分层自动化测试体系**（单测 → 集成测试 → 组件测试 → 契约测试 → E2E）。
>
> **核心策略：绞杀者模式（Strangler Fig），不推倒重来。**
> 每个阶段结束时代码都必须**可编译、可运行、可发布、可回滚**；任何时刻 `main` 分支都是可交付状态。
> 严禁出现"重构中途分支"长期不可用的情况。

---

## 目录

- [0. 功能基线清单（重构零丢失的验收依据）](#0-功能基线清单重构零丢失的验收依据)
- [1. 目标架构](#1-目标架构)
- [2. 重构原则与硬约束](#2-重构原则与硬约束)
- [3. 分阶段改造清单](#3-分阶段改造清单)
  - [Phase 0 工具链地基](#phase-0-工具链地基不改行为)
  - [Phase 1 止血与一致性](#phase-1-止血与一致性行为修复)
  - [Phase 2 Rust 抽纯领域层](#phase-2-rust-抽纯领域层-domain)
  - [Phase 3 Rust 平台抽象与状态集中](#phase-3-rust-平台抽象与状态集中)
  - [Phase 4 Rust 基础设施与安全加固](#phase-4-rust-基础设施与安全加固)
  - [Phase 5 前端基础设施](#phase-5-前端基础设施)
  - [Phase 6 前端纯逻辑下沉](#phase-6-前端纯逻辑下沉-model)
  - [Phase 7 前端组件拆分](#phase-7-前端组件拆分)
  - [Phase 8 状态与持久化统一](#phase-8-状态与持久化统一)
  - [Phase 9 自动化测试体系](#phase-9-自动化测试体系)
  - [Phase 10 发布工程与文档治理](#phase-10-发布工程与文档治理)
- [4. 工作量估算与排期](#4-工作量估算与排期)
- [5. 风险登记册与缓解措施](#5-风险登记册与缓解措施)
- [6. 回滚策略](#6-回滚策略)
- [7. 完成的定义（DoD）](#7-完成的定义dod)

---

## 0. 功能基线清单（重构零丢失的验收依据）

> **规则**：下列每一项都是当前可观察行为。重构期间**每个 Phase 结束后必须全量回归勾选**，
> 任一勾不上即视为该 Phase 失败，禁止进入下一 Phase。
> 本清单将同步落地为 E2E 冒烟用例（见 `TEST_PLAN.md` §5）。

### A. 宠物窗口（main）

- [ ] A1 启动后宠物常驻屏幕右下角，避开 macOS Dock（底边距 75px）/ Windows 任务栏（64px），右边距 24px
- [ ] A2 窗口无边框、透明、置顶、不抢焦点、不出现在 Dock / Cmd+Tab
- [ ] A3 默认缩放 70%，宠物帧 192×208 按 scale 渲染
- [ ] A4 空闲时每 6~15s 随机播放一次闲时动作（wave/jump/failed/waiting/working/look 中该宠物具备的动作）
- [ ] A5 单击宠物 → 随机动作 + 对应性格台词气泡，3s 后消失
- [ ] A6 悬停宠物 → 播放 `waiting`，移出 → 回 idle
- [ ] A7 拖动宠物 → 按真实水平位移方向播放 `runningLeft` / `runningRight`，松手回 idle
- [ ] A8 双击宠物 → 打开设置窗口
- [ ] A9 `talk` 状态下拖动不触发随机动作（让位给气泡）
- [ ] A10 透明区域点击穿透到下层桌面/窗口；宠物与气泡区域可交互（macOS：NSTimer；Windows：SetWindowRgn）
- [ ] A11 气泡阴影未被穿透裁切除掉（macOS/Windows 双端）
- [ ] A12 缩放变化后窗口尺寸跟随，以**右下角为锚点**不漂移

### B. 通知能力

- [ ] B1 外部 HTTP `POST http://127.0.0.1:8756/notify` → 气泡显示，默认 4s 消失
- [ ] B2 `action` 字段指定动作时：先播动作，再显示气泡
- [ ] B3 多条通知排队依次播放，不互相覆盖
- [ ] B4 `text` 超过 120 字 → 400 + JSON error 字段
- [ ] B5 `text` 为空 / 非法 JSON / 非 POST / 非 `/notify` → 对应错误码
- [ ] B6 设置窗口「测试通知」走 IPC（`push_notify`），与 HTTP 共用同一气泡
- [ ] B7 测试通知弹窗 3 秒发送冷却倒计时
- [ ] B8 通知文案可滚动（多行）、可复制 curl 示例

### C. 设置窗口（settings）

- [ ] C1 双击宠物 / 托盘「打开设置」/ 托盘左键点击 → 打开设置窗口
- [ ] C2 首次打开居中，之后恢复到上次拖动位置（关闭时隐藏而非销毁）
- [ ] C3 宠物列表：内置 3 只 + 已导入外部宠物，点击切换，选中态高亮
- [ ] C4 缩放滑块 50%~130%，步进 0.05，拖动实时生效并同步到 main 窗口
- [ ] C5 「显示宠物」开关 → 联动整个 main 窗口显隐，状态持久化
- [ ] C6 本地导入 `.zip` → 解压、校验、注册、自动切换到新宠物、气泡提示
- [ ] C7 外部宠物支持编辑（名字/描述）与删除（二次点击确认，3s 超时复位）
- [ ] C8 在线画廊：浏览（搜索名字/作者/分类）、下载、已安装显示「重新下载」、下载后自动切换
- [ ] C9 左下角显示版本号（取自 `tauri.conf.json`）
- [ ] C10 窗口任意空白处可拖动，交互元素（按钮/输入框/滑块/卡片）不触发拖拽
- [ ] C11 托盘 ↔ 设置窗口 ↔ main 窗口三端状态实时同步（切宠物 / 缩放 / 显隐）
- [ ] C12 禁用右键菜单

### D. 托盘

- [ ] D1 托盘图标 + tooltip「PetBuddy」
- [ ] D2 菜单项：打开设置 / 显示隐藏宠物 / 开机自启（勾选态）/ 切换宠物（内置+外部+更多设置）/ 退出
- [ ] D3 导入/删除/下载宠物后托盘「切换宠物」子菜单自动重建
- [ ] D4 开机自启：macOS `SMAppService` 登录项；Windows HKCU Run 键（带 `--autostart` 标记）

### E. 外部宠物兼容

- [ ] E1 zip 内含 `pet.json` + `spritesheet.webp` 即可导入
- [ ] E2 非标准行数（如 9 行）的精灵图：越界 `row` 的动作被移除、`count` 被截断，宠物不消失
- [ ] E3 重启后已导入宠物自动恢复（`list_imported_pets`）
- [ ] E4 非法 id（空 / 含 `..` / 非 ASCII 字母数字）被拒绝；zip 内 `../` 路径不逃逸

---

## 1. 目标架构

### 1.1 Rust 侧（关键变化：新增 `lib.rs` + `domain` 纯逻辑层 + `platform` trait 抽象）

```
src-tauri/
├── Cargo.toml                  # ★ 新增 [lib] petbuddy_lib，使集成测试可引用
├── src/
│   ├── main.rs                 # 入口，仅 3 行：petbuddy_lib::run()
│   ├── lib.rs                  # ★ 新增：pub mod + run()
│   ├── error.rs                # ★ 新增：AppError / ErrorCode 统一错误
│   ├── app/                    # 装配层（从 main.rs 拆出）
│   │   ├── mod.rs
│   │   ├── builder.rs          # tauri::Builder 组装 + invoke_handler
│   │   ├── setup.rs            # setup hook（按平台分派）
│   │   ├── tray.rs             # 托盘菜单构建 / 菜单事件
│   │   └── window_events.rs    # settings 关闭/移动事件
│   ├── commands/               # ★ 命令层：薄，只做反序列化 + 校验 + 分派
│   │   ├── mod.rs              # 命令名常量集中（前端同步生成）
│   │   ├── pet.rs              # import/list/delete/update/browse/download
│   │   ├── notify.rs           # push_notify
│   │   ├── window.rs           # resize/open/close/hide/interactive rects
│   │   └── system.rs           # autostart/open_external/quit/platform
│   ├── domain/                 # ★★ 纯领域逻辑：零 Tauri 依赖 → 100% 可单测
│   │   ├── mod.rs
│   │   ├── geometry.rs         # ← 迁移自现有 geometry.rs
│   │   ├── layout.rs           # ★ pet_window_size / 右下角锚点 / 气泡区高度
│   │   ├── pet/
│   │   │   ├── mod.rs
│   │   │   ├── model.rs        # PetDef / FrameSeq / Frame（★ 新增 per-pet frame）
│   │   │   ├── codec.rs        # ★ webp_dimensions / base64 / zip 解包（纯函数）
│   │   │   └── validator.rs    # id 白名单 / clamp_seq / 路径穿越检查
│   │   ├── notify/
│   │   │   ├── mod.rs
│   │   │   ├── model.rs        # NotifyPayload
│   │   │   ├── http_request.rs # ★ &[u8] -> Result<Request>（纯解析）
│   │   │   └── http_response.rs# ★ 响应构造
│   │   └── gallery/
│   │       ├── mod.rs
│   │       └── index.rs        # pets.json 映射 / 名称回退链（纯）
│   ├── platform/               # ★ 平台抽象：trait + cfg 实现
│   │   ├── mod.rs              # pub use 当前平台实现（编译期零成本）
│   │   ├── traits.rs           # trait HitTest / AutoStart / WindowChrome
│   │   ├── macos/
│   │   │   ├── mod.rs
│   │   │   ├── hit_test.rs     # ← macos_pet.rs（NSTimer + 穿透 + 原生拖拽）
│   │   │   ├── activation.rs   # ActivationPolicy 管理
│   │   │   └── autostart.rs    # SMAppService
│   │   ├── windows/
│   │   │   ├── mod.rs
│   │   │   ├── region.rs       # ← windows_pet.rs（SetWindowRgn）
│   │   │   ├── dwm.rs          # 圆角 + 阴影
│   │   │   └── autostart.rs    # HKCU Run
│   │   └── linux/
│   │       └── mod.rs          # no-op 实现 + 友好日志
│   ├── infra/                  # 外部 IO 适配（可注入 → 可测试）
│   │   ├── mod.rs
│   │   ├── http_server.rs      # TcpListener（薄，调 domain 解析器）
│   │   ├── http_client.rs      # ★ reqwest 封装（timeout + 连接复用）
│   │   ├── storage.rs          # ★ app_data_dir 读写（根路径可注入）
│   │   └── archive.rs          # zip 解包（纯 IO）
│   ├── state/                  # ★ 集中状态（收编 8 个散落 static Mutex）
│   │   ├── mod.rs              # AppState
│   │   ├── hit_rects.rs
│   │   └── settings_pos.rs
│   └── bindings/               # ★ ts-rs 生成的 TS 类型（契约测试用）
│       └── mod.rs
└── tests/                      # ★ 集成测试
    ├── http_server.rs
    ├── pet_storage.rs
    └── zip_slip.rs
```

### 1.2 前端侧（关键变化：`shared` / `features` 垂直切分 + `windows` 装配层）

```
src/
├── main.ts
├── App.vue                     # 仍按 windowLabel 路由，但只做分发（<30 行）
├── shared/                     # 与业务无关的通用能力
│   ├── config/
│   │   ├── constants.ts        # ★★ 所有魔数唯一真源（帧尺寸/缩放/端口/上限/超时）
│   │   └── runtime.ts          # isTauri / windowLabel
│   ├── platform/
│   │   ├── index.ts            # ★ getPlatform()（由 Rust 提供，消灭 UA 嗅探）
│   │   └── types.ts
│   ├── ipc/
│   │   ├── client.ts           # ★ 类型化 invoke + 统一错误 + 可选超时
│   │   ├── events.ts           # ★ 事件总线（source 防回环 + 自动清理）
│   │   └── commands.ts         # 命令名常量（与 Rust 同步生成）
│   ├── errors/
│   │   ├── AppError.ts
│   │   └── messages.ts         # 错误码 → 用户可读文案
│   ├── utils/
│   │   ├── base64.ts
│   │   ├── throttle.ts
│   │   └── assert.ts
│   └── styles/
│       ├── tokens.css          # ★★ 设计令牌唯一真源（色值/圆角/阴影/字号）
│       ├── base.css
│       └── reset.css
├── features/                   # 按功能域垂直切分
│   ├── pet/
│   │   ├── model/              # ★ 纯逻辑（零 Vue 依赖 → 100% 可单测）
│   │   │   ├── types.ts
│   │   │   ├── frame.ts        # 帧序计算 / 越界裁剪
│   │   │   ├── actionScheduler.ts  # 动作状态机（play/random/queue 纯 reducer）
│   │   │   └── geometry.ts     # 命中矩形 + 阴影 padding 推导
│   │   ├── store/usePetStore.ts
│   │   ├── api/petApi.ts
│   │   ├── composables/
│   │   │   ├── usePetActions.ts
│   │   │   ├── usePetDrag.ts       # ★ 平台差异在此隔离
│   │   │   └── useHitRects.ts
│   │   └── components/
│   │       ├── SpritePet.vue
│   │       ├── PetStage.vue
│   │       └── PetBubble.vue
│   ├── notify/
│   │   ├── model/{types.ts,notifyQueue.ts}   # ★ 队列/去重/duration 纯函数
│   │   ├── store/useNotifyStore.ts
│   │   ├── api/notifyApi.ts
│   │   └── composables/useNotifyQueue.ts
│   ├── gallery/
│   │   ├── model/filter.ts     # ★ 搜索过滤纯函数
│   │   ├── api/galleryApi.ts
│   │   └── components/{OnlineGalleryDialog.vue,GalleryCard.vue}
│   ├── import/
│   │   ├── model/validate.ts
│   │   ├── api/importApi.ts
│   │   └── components/{ImportButton.vue,PetEditDialog.vue}
│   └── settings/
│       ├── store/useSettingsStore.ts
│       └── components/
│           ├── SettingsShell.vue
│           ├── PetListPanel.vue
│           ├── PetPreviewPanel.vue
│           ├── ScaleSlider.vue
│           ├── ToggleField.vue
│           └── NotifyTestDialog.vue
├── windows/                    # 窗口级装配层（只做编排，不含业务逻辑）
│   ├── PetWindow.vue           # ← PetHost.vue（763 行 → <200 行）
│   └── SettingsWindow.vue      # ← PetSettings.vue（1635 行 → <150 行）
├── composables/
│   ├── useTauriEvent.ts        # ★ 自动注册 + unmount 自动取消（修 P1-1）
│   └── useWindowDrag.ts
└── test/
    ├── setup.ts
    └── mocks/tauri.ts          # ★ IPC mock（使全部逻辑可在 jsdom 下单测）
```

### 1.3 架构分层依赖规则（由 ESLint `import/no-restricted-paths` 强制）

```
windows/      → features/ → shared/          ✅ 允许
features/     → shared/                      ✅ 允许
model/        → shared/config, shared/utils  ✅ 仅允许（禁止 import vue / ipc）
shared/ipc    → shared/errors, shared/config ✅ 仅允许
model/        → ipc/                         ❌ 禁止（保证纯逻辑可测）
features/A    → features/B/model             ⚠️ 仅允许 model 层，需显式登记
任何层        → src-tauri（直接 invoke）      ❌ 禁止，必须走 shared/ipc
```

---

## 2. 重构原则与硬约束

| #   | 原则                       | 说明                                                                                                                               |
| --- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| R1  | **先抽纯逻辑，再动结构**   | 任何模块重构前，先把其中不依赖 IO/平台的纯函数抽到 `domain/` 或 `model/` 并补单测。有测试兜底才动结构。                            |
| R2  | **一个 PR 一件事**         | 每个任务项一个独立 commit/PR，禁止"重构 + 修 bug + 加功能"混在一起。                                                               |
| R3  | **格式化提交独立**         | 全量格式化必须是**单独的 commit**，并写入 `.git-blame-ignore-revs`，否则 `git blame` 全毁。                                        |
| R4  | **行为不变优先于结构优美** | 若某项重构会改变可观察行为，必须先标注为「行为变更」并在功能基线上更新，经确认后再做。                                             |
| R5  | **平台分支收敛到一处**     | Rust 用 `#[cfg]` + trait；前端用 `platform/index.ts`。业务代码中禁止出现 `isMac` / `navigator.platform` / `cfg(target_os)`。       |
| R6  | **常量单一真源**           | 任何跨语言共享的常量（帧尺寸、缩放范围、端口、字数上限）必须由 Rust 生成到前端（ts-rs 或 JSON 快照 + CI 校验），禁止两边各写一份。 |
| R7  | **错误不吞**               | 禁止裸 `let _ = ...` 吞错误；所有 IPC 失败必须有日志或用户可见提示。                                                               |
| R8  | **测试先行于删除**         | 删除任何"看起来没用"的代码前，先确认其在功能基线中无对应项；`--pet-w` 这类死代码删除需在同一 PR 内验证无引用。                     |

---

## 3. 分阶段改造清单

> 图例：`[ ]` 待办　`★` 关键路径　`⚠️` 行为变更（需确认）　`🧪` 必须同步补测试

---

### Phase 0 工具链地基（不改行为）

**目标**：在动任何业务代码前，先把"质量基础设施"装好。**本阶段零行为变化。**

- [x] **0.1** 新增 `rustfmt.toml`
  ```toml
  edition = "2021"
  max_width = 100
  ```
- [x] **0.2** 新增 `clippy.toml` + 在 CI 中执行 `cargo clippy --all-targets -- -D warnings`
- [x] **0.3** 新增 ESLint 9 flat config `eslint.config.js`
  - `@eslint/js` + `typescript-eslint` + `eslint-plugin-vue`（`flat/recommended`）
  - 启用 `vue/multi-word-component-names` off（本项目单文件组件名合法）
  - **启用 `import/no-restricted-paths` 落地 §1.3 依赖规则**
  - 启用 `vue/max-attributes-per-line`（`singleline: 3`）
- [x] **0.4** 新增 `.prettierrc` + `.prettierignore`（忽略 `dist/`、`src-tauri/target/`、`src/bindings/`）
- [x] **0.5** 新增 `.editorconfig`（`end_of_line = lf`、`charset = utf-8`、`insert_final_newline = true`）
- [x] **0.6** `package.json` 新增脚本
  ```jsonc
  "lint":      "eslint . --max-warnings 0",
  "lint:fix":  "eslint . --fix",
  "format":    "prettier --write . && cargo fmt",
  "format:check": "prettier --check . && cargo fmt --check",
  "typecheck": "vue-tsc --noEmit",           // 已存在
  "test":      "vitest run",
  "test:watch":"vitest",
  "test:cov":  "vitest run --coverage",
  "test:rust": "cargo test --manifest-path src-tauri/Cargo.toml",
  "verify":    "npm run typecheck && npm run lint && npm run test && npm run test:rust"
  ```
- [x] **0.7** 安装前端测试依赖（devDependencies）
  - `vitest`、`@vue/test-utils`、`jsdom`、`@vitest/coverage-v8`、`@vitest/ui`
- [x] **0.8** 新增 `vitest.config.ts`（environment: jsdom，setupFiles: `src/test/setup.ts`，覆盖率 provider: v8）
- [x] **0.9** 新增 CI `.github/workflows/quality.yml`（**设为 required status check**）
  ```yaml
  on: [push, pull_request]
  jobs:
    frontend: typecheck → lint → format:check → test:cov（上传覆盖率）
    rust: cargo fmt --check → cargo clippy -D warnings → cargo test
  ```
- [ ] **0.10** ★ **独立格式化提交**：`npm run lint:fix && npm run format`，单独一个 commit
  > 状态：格式化内容已生成在工作区，待提交人切出独立 commit。
- [x] **0.11** 新增 `.git-blame-ignore-revs` 登记 0.10 的 commit，并 `git config blame.ignoreRevsFile .git-blame-ignore-revs`
  > 状态：文件已建好（含操作说明），**待 0.10 提交后把其 hash 追加到文件末尾**。
- [x] **0.12** 新增 `docs/ARCHITECTURE.md`（当前架构快照 + 本次重构目标架构 + 变更记录）

**✅ Phase 0 验收**：`npm run verify` 全绿；CI quality job 通过且为 required；`git blame` 仍可追溯到格式化前的真实作者。

> **实际落地记录（0.1~0.9 / 0.12 已完成）**
>
> - `rustfmt.toml`（max_width=100，与 Prettier 对齐）、`clippy.toml`（msrv=1.77.0）
> - `eslint.config.js`：ESLint 10 + typescript-eslint 8 + eslint-plugin-vue 10 + **eslint-config-prettier 收尾**
>   （排版类规则全部交给 Prettier，避免两套工具打架——这是首次 lint 出现 50 条排版告警后的关键调整）
> - `.prettierrc` / `.prettierignore` / `.editorconfig`
> - `package.json` 增加 `engines: { node: ">=20.19" }` 与 12 条脚本，`verify` 串起全链路
> - `vitest.config.ts` + `src/test/{setup,helpers}.ts` + `src/test/fixtures/Hello.vue`
> - `src/test/harness.spec.ts`：**7 条测试基础设施自检用例**（canvas mock / Vue 挂载 / Image mock / flushPromises），
>   用于证明安全网本身可用，而不是只配置了一堆文件
> - CI `quality.yml`：`frontend`（typecheck/lint/format/test:cov）+ `rust`（fmt/clippy/test，Linux）
>   - `rust-windows`（clippy，覆盖 Windows 专属 cfg 分支）
>
> **附带完成（让 `-D warnings` 门禁可落地的必要修复，均为零行为变更）**
>
> clippy 首次运行报 11 处告警，已全部修复：
> `geometry.rs` 改用 `f64::clamp`、`pet_import.rs` 4 处（doc 缩进 / `div_ceil` / 去 `Ok(def?)` / 去多余借用）、
> `main.rs` 3 处（2 处 `if let` 折叠为 `Ok(Some(mon))`、macOS+Windows 分支都改 / `let _ =` 去单元值）、
> `macos_pet.rs` 用 `c"c@:"` C 字符串字面量、`notify_server.rs` 2 处（去 `as u64` 冗余转换 / `as_bytes().len()` → `len()`）。
>
> **当前 `npm run verify` 结果**：typecheck ✓ / eslint ✓ / prettier ✓ / cargo fmt ✓ / vitest 7 passed ✓ / cargo test 3 passed ✓

---

### Phase 1 止血与一致性（行为修复）

**目标**：清掉 Review 中的 P0/P1 缺陷。**每项一个 PR，每项带回归测试。**

- [x] **1.1** 🧪 **P0-1 外部宠物 per-pet frame 支持（Rust 侧产出）**
  - `RawPetJson` 增加 `frame: Option<RawFrame { width, height, cols }>`
  - `build_pet_def` 用 webp 实际尺寸 + 声明 frame 计算 `rows/cols`，返回 `PetDefJson.frame`
  - **验收**：192×208/8列 与 非标尺寸（如 256×256/6列）两类包均能正确切帧
- [x] **1.2** 🧪 **P0-2 修 `download_online_pet` 目录名与 id 不一致**
  - 强制 `raw.id = slug.clone()`（或目录名改从 `raw.id` 派生并走同一白名单校验）
  - **回归用例**：构造 `pet.json` 中 `id != slug` 的远端响应 → 下载 → 删除 → 断言目录确实被删除
- [x] **1.3** 🧪 **P0-3 通知服务安全加固**（详见 Phase 4.2，可提前）
  - 立即版：Host 头校验 + `Content-Length` 上限 8KB + 总超时 10s
- [x] **1.4** 🧪 **P0-4 `duration` 参数端到端生效**
  - `NotifyItem` 增加 `duration?: number`；`showNotify` 用 `item.duration ?? DEFAULT_BUBBLE_MS`
  - **回归用例**：`{duration: 1000}` → 1s 后气泡消失（用 vi.useFakeTimers）
- [x] **1.5** 🧪 **P0-5 macOS NSTimer tick 健壮性**
  - tick 整体包 `catch_unwind(AssertUnwindSafe(...))`
  - 所有 `Mutex::lock().unwrap()` → `if let Ok(g) = ... else { return; }`
- [x] **1.6** 🧪 **P1-1 `PetHost` 事件监听泄漏**
  - 抽出 `useTauriEvent(name, handler)` composable，内部 `onMounted` 注册、`onUnmounted` 取消
  - **回归用例**：mount → unmount → 断言所有监听被取消（mock `listen` 返回的 un 被调用 N 次）
- [x] **1.7** 🧪 **P1-2 `reqwest` 超时**
  - `Client::builder().timeout(Duration::from_secs(15)).connect_timeout(5s)`
  - 客户端提取为 `OnceLock<Client>` 复用连接池
- [x] **1.8** 🧪 **P1-3 缩放闪空白帧**：`displayScale` watch 改完 canvas 尺寸后立即 `drawFrame(seq.row, frameIdx)`
- [x] **1.9** 🧪 **P1-4 切宠物残影**：`loadImage()` 开头 `ctx.clearRect` + 置 `curSeqKey = ''`
- [x] **1.10** **P1-5 `pushNotify` 未处理的 Promise**
  - `PetSettings` 中 6 处调用统一改为 `void pushNotify(...).catch(e => showError(e))`
  - 加 ESLint 规则 `@typescript-eslint/no-floating-promises`
- [x] **1.11** **P1-6 文案乱码 + 文案纠错**
  - `气ß泡` → `气泡`；同时把提示文案改为与实际一致（气泡 4s 自动消失）
  - 加 CI 检查：源码中出现 `ß` 等异常字符告警（可选）
- [x] **1.12** **死代码清理**
  - 删 `--pet-w` / `--tail-right` 及 `petWidth` / `petWidthCss` / `tailRightCss`
  - 删 `import_pet` 的 `file_name` 参数与 `let _ = &file_name;`
  - 删 `updateExternalPet` 中无意义的 localStorage 写入
- [x] **1.13** **注释/文档漂移修正**
  - `main.rs:65` 注释 `0.8~1.3` → `0.5~1.3`
  - README：Windows 自启改为注册表 Run 键；NSTimer 50ms → 16ms；Info.plist 描述与 `tauri.conf.json` 对齐
- [x] **1.14** 🧪 **【新增】`PetHost` 的 notify watcher 脱离组件作用域导致泄漏**
  - 现象：写通知气泡测试时发现「第 1 个用例能收到通知、后续用例收不到」
  - 根因：`watch(notifyStore.pending)` 注册在 **`async onMounted` 的 `await nextTick()` 之后**。
    `await` 之后恢复执行时 Vue 的 `currentInstance` 已重置为 null，该 watcher 脱离组件的
    effect scope，**组件 unmount 时不会被停止**，永久存活并持续抢消费 `notifyStore.pending`
  - 修复：把该 watch 移到 `<script setup>` 顶层注册，由组件 scope 托管
  - 回归用例：`PetHost.spec.ts › unmount 后不再消费通知`
  - 影响面：生产环境 PetHost 只在 main 窗口挂载一次、生命周期等同 App，故未暴露；
    但 HMR 与「窗口重建」场景下会累积多个消费者，属于真实缺陷

> **Phase 1 已完成部分（1.4 / 1.8 / 1.9 / 1.11 / 1.12 / 1.13 / 1.14）**
>
> - **P0-4 `duration` 端到端生效**：`NotifyItem` 增加 `duration`，`showNotify` 用
>   `normalizeDuration()` 取值。同时加了健壮性规则——非数字 / ≤0 / `NaN` 回退默认 4000ms，
>   超过 60s 截断（HTTP 侧 duration 是任意 u64，不设上限会让气泡永久占用）。
>   README 已补充取值规则表。
> - **P1-3 缩放不闪空白帧**：`displayScale` watch 在改完 `canvas.width/height` 后立即重绘当前帧。
> - **P1-4 切宠不残影**：`loadImage()` 开头调 `clearCanvas()`。
> - **P1-6 文案乱码**：`气ß泡` → `气泡`；同时把「双击气泡或 3 条后会自动消失」这句
>   **不实描述**改为「气泡默认 4 秒后自动消失，多条通知会依次排队播放」。
> - **死代码清理**：`--pet-w` / `--tail-right` 及三个 computed；`import_pet` 的 `file_name`
>   参数（含前端调用点同步改）；`updateExternalPet` 中无意义的 localStorage 写入。
> - **注释/文档漂移**：见 1.13 三项。
>
> **新增回归测试 10 条**（`SpritePet.spec.ts` 4 条 + `PetHost.spec.ts` 6 条），
> 全量 `npm run verify` 通过：前端 17 passed / Rust 3 passed。

> ### 追加完成：1.2 / 1.6
>
> **1.2 P0-2 目录名与 id 不一致**
> 抽出纯函数 `normalize_pet_id_json(json_text, slug)`：把远程 pet.json 的 `id`
> 改写为 slug 并返回规范化文本，**且落盘写回的是规范化后的文本**。
> 修复前只改内存中的 `raw`、写盘的仍是原文，导致重启后 `list_imported_pets`
> 读回远程 id，删除/编辑去拼一个不存在的目录——因 `if target.exists()` 保护而
> **静默成功却什么都没做**。新增 8 条 Rust 单测，含一条显式断言
> 「id == 本地目录名」契约的用例，防止将来被改回。
>
> **1.6 P1-1 事件监听泄漏**
> 新增 `composables/useTauriEvent.ts`，把「注册 + 卸载自动取消 + 处理
> `listen()` 异步竞态（组件先卸载、listen 后 resolve 时立即取消）」收敛到一处。
> `PetHost` 的 9 个监听、`PetSettings` 的 2 个监听全部改用它，
> 并顺手修掉两处 `document.addEventListener('contextmenu', ...)` 从未移除的泄漏。
> 新增 4 条 composable 回归用例。
>
> **当前测试规模**：前端 21 passed（4 个 spec 文件）/ Rust 11 passed。

> ### 追加完成：1.3（P0-3 通知服务安全加固）
>
> `notify_server.rs` 重写，补上五道护栏，并把校验逻辑全部做成**纯函数**以便单测：
>
> | 防护                       | 实现                                                                                   | 常量                     |
> | -------------------------- | -------------------------------------------------------------------------------------- | ------------------------ |
> | DNS-rebinding / 浏览器跨站 | `Host` 头白名单（仅 `127.0.0.1` / `localhost` / `::1`，端口不参与比较）→ 403           | —                        |
> | 请求体过大                 | `Content-Length` 超限 → 413（在读 body **之前**判定）                                  | `MAX_BODY_BYTES = 8KB`   |
> | 请求头过大                 | header 未收完即超限 → 431                                                              | `MAX_HEADER_BYTES = 8KB` |
> | 慢速攻击                   | 单连接总截止时间（原 `set_read_timeout` 只约束单次 read，可被 1 字节/次无限续期）→ 408 | `TOTAL_TIMEOUT = 10s`    |
> | 连接打满                   | 并发计数 + Drop 守卫（panic 也不会漏减）→ 超限直接关闭                                 | `MAX_CONNECTIONS = 32`   |
>
> 另修一处可观测性缺陷：端口被占用时原本只 `eprintln!`，
> 用户只看到「通知发不出去」却不知原因；现改为 `emit("notify-server-error")`。
>
> 新增 13 条 Rust 单测（`notify_server::tests`），含 `127.0.0.1.evil.com`
> 这类「形似回环实为外部域名」的负例。
> README 补充错误响应码表与安全边界说明。
>
> **当前测试规模**：前端 21 passed / Rust 24 passed。

> ### 追加完成：1.1（P0-1 外部宠物 per-pet frame 支持）
>
> 之前「帧尺寸」是写死的全局常量（192×208、8 列），所有宠物共用，
> 非标准外部包（高清帧、或 256×256/6 列的包）切帧错位。
>
> 变更：
>
> - `PetDefJson` 新增 `frame: { width, height, cols, rows }`；`RawPetJson` 新增
>   可选 `frame: { width, height, cols }`。
> - 新增 `compute_frame()`：声明优先，rows 由精灵图实际高度 / 帧高推导；
>   未声明则用默认 192×208/8 列且 rows 由 sheet 高 / 208 推导。
> - `clamp_seq()` 改用该宠物的**真实列数**而非写死的 `FRAME_COLS`，
>   修掉了「17 列高清包的帧被截断到 8 列」的隐藏 bug。
> - 前端：PetDef 加 `frame` 字段（可选，内置宠物走全局 petStore.frame）；
>   `SpritePet` 改为 `currentPet.frame ?? petStore.frame`，外部宠物按自己的
>   几何切帧。
>
> 新增 7 条 Rust 单测，覆盖验收两类包（192×208/8、256×256/6）及 17 列截断等。
> 注：为单测构造的 `make_webp` 已与 `webp_dimensions` 的真实解析偏移对齐
> （该函数从 data[20] 起读 24 位宽/高）。
>
> ### 追加完成：1.5 / 1.7 / 1.10
>
> **1.5 macOS NSTimer tick**：`tick` 逻辑抽到 `tick_inner()`（返回 `Option<()>`，
> 锁中毒即提前 return），新增 `lock_ok()` 取锁助手把 hot-path 上的 `.unwrap()`
> abort 风险转为「跳过本次 tick」；`tick` 方法体包 `catch_unwind`，跨 FFI 边界
> panic 不再直接 abort 整个进程。新增 2 条 `lock_ok` 单测。
>
> **1.7 reqwest 超时**：抽出 `http_client()`（OnceLock 共享，连接复用 +
> connect_timeout 5s / timeout 15s），替换两处各自新建且**无超时**的客户端
> （reqwest 默认不超时，网络挂起时画廊永久卡 loading）。新增 1 条复用单测。
>
> **1.10 pushNotify 未处理 Promise**：`PetSettings` 新增 `notify()` 包装，
> 6 处裸 `pushNotify(...)` 改为走包装，失败仅 console.error 不打断流程。
>
> **当前测试规模**：前端 21 passed / Rust 33 passed。

**✅ Phase 1 验收**：功能基线 A/B/C/D/E 全量回归通过；P0 五项全部关闭；新增 ≥20 个回归测试。

---

### Phase 2 Rust：抽纯领域层（`domain/`）

**目标**：把散落在 `main.rs` / `pet_import.rs` / `notify_server.rs` 中的**纯计算**全部抽到 `domain/`，
这是整个重构**收益最大**的一步——抽出来的代码立刻可 100% 单测。

- [x] **2.1** ★ 新增 `src/lib.rs` + `Cargo.toml` 的 `[lib]`
  ```toml
  [lib]
  name = "petbuddy_lib"
  crate-type = ["staticlib", "cdylib", "rlib"]
  path = "src/lib.rs"
  ```
  `main.rs` 改为 `fn main() { petbuddy_lib::run() }`
  **这是 Tauri 官方推荐的集成测试前置条件**
- [x] **2.2** 迁移 `geometry.rs` → `domain/geometry.rs`（内容基本不变，仅移动 + 补充测试）
  - 🧪 补：`NaN` / `inf` / 负尺寸矩形 / 重叠矩形 / `scale=0`
- [x] **2.3** ★ 新增 `domain/layout.rs`
  - 抽出 `pet_window_size(scale) -> (w, h)`（从 `main.rs:71`）
  - 抽出 `anchor_bottom_right(old_pos, old_size, new_w, new_h, scale_factor) -> (x, y)`（从 `main.rs:101-110` 的内联计算）
  - 抽出常量：`FRAME_W/H`、`BUBBLE_ZONE_H`、`WINDOW_PAD`、`EDGE_GAP`、**`BASE_WINDOW_W`**
  - 🧪 补：scale=0.5/0.7/1.0/1.3 的黄金值快照；锚点计算在多次连续 resize 后右下角不漂移
- [x] **2.4** ★ 新增 `domain/pet/codec.rs`（纯函数，输入 `&[u8]`，输出 `Result`）
  - `webp_dimensions`（从 `pet_import.rs:72`）
  - `base64_encode` / `base64_decode`（从 `pet_import.rs:162/179`）
  - 🧪 补：
    - VP8 / VP8L / VP8X 三种真实文件头样本（`tests/fixtures/*.webp`，用 `include_bytes!`）
    - **回归用例：VP8 宽度 ≥ 16384**（`pet_import.rs:82-84` 记录的历史 bug）
    - base64 roundtrip：长度 0/1/2/3/0x8000+1 的边界
    - 非 webp / 截断到 12 字节 / 空输入 → `None`
- [x] **2.5** ★ 新增 `domain/pet/validator.rs`
  - `is_valid_pet_id`（从 `pet_import.rs:321-326` 的三处重复正则）
  - `clamp_seq`（从 `pet_import.rs:129`）
  - `safe_join(root, id)`：返回 `Result<PathBuf>` 的穿越检查
  - 🧪 补：id 白名单正反例各 6 个；`../`、`/etc`、空串、Unicode；`clamp_seq` 全覆盖
- [x] **2.6** ★ 新增 `domain/pet/model.rs`
  - `PetDef` / `FrameSeq` / **`Frame { width, height, cols }`** / `PetDefJson`
  - `build_pet_def(raw, bytes) -> PetDefJson`（从 `pet_import.rs:217`，改为不碰文件系统）
  - 🧪 补：11 行图 / 9 行图 / 不可解析→回退 / per-pet frame / actions 越界被移除
- [x] **2.7** ★ 新增 `domain/notify/http_request.rs` + `http_response.rs`
  - `parse_request(&[u8]) -> Result<Request, HttpError>`（从 `notify_server.rs:76-140`）
  - `find_subslice` / Content-Length 解析 / body 边界判定全部纯函数化
  - `render_response(HttpError) -> Vec<u8>`：状态码与 body 构造
  - 🧪 补（**本阶段测试重点**）：
    - 完整请求 / **TCP 分段（header 与 body 分两次到）** / 无 Content-Length
    - Content-Length 大于实际 / 小于实际
    - 非 POST、非 `/notify` → 404；非法 JSON → 400；text 空 → 400
    - **text 121 字中文 → 400 且 error 文案正确**（按 chars 计数，回归中文计数逻辑）
    - Host 头非 `127.0.0.1:*` → 403；body > 8KB → 413
- [x] **2.8** 新增 `domain/gallery/index.rs`
  - `map_online_pets(Vec<RawOnlinePet>) -> Vec<OnlinePetMeta>`（名称回退链：`zh → en → name → slug`）
  - `preview_url(slug)` / `pet_json_url(slug)` / `spritesheet_url(slug)`
  - 🧪 补：回退链 4 级全覆盖；slug 为空被跳过
- [x] **2.9** ★ 新增 `error.rs`：`AppError { code: ErrorCode, message: String }`
  - `ErrorCode` 枚举：`InvalidPetId` / `ZipSlip` / `TooLarge` / `BadRequest` / `Network` / `Io` / `Platform` / `Serialization`
  - 实现 `From<std::io::Error>` / `From<serde_json::Error>` / `From<reqwest::Error>`
  - 所有 command 返回 `Result<T, AppError>`，前端按 `code` 映射文案
- [x] **2.10** 收敛重复 IPC：`update_interactive_rects` 与 `set_pet_hit_rects` 在前端只调前者
  - 保留旧命令作兼容（标注 `#[deprecated]`），前端 `PetHost` 去掉 `setPetHitRects` 调用
  - 保留 `apply_pet_hit_rects`（Windows 需要显式 apply）
  - ✅ 已完成：前端 `PetHost` 仅调 `updateInteractiveRects` + `applyPetHitRects`，`setPetHitRects` 调用已移除；Rust `set_pet_hit_rects` 用 doc `@deprecated` 标注（不用 `#[deprecated]` 属性，因 `generate_handler!` 展开处会触发 `-D warnings`）

**✅ Phase 2 验收**：`cargo test` 新增 ≥60 个单测；`domain/` 模块**零 `tauri::` import**（用 grep 断言）；功能基线全绿。

---

### Phase 3 Rust：平台抽象与状态集中

**目标**：消灭业务代码里的 `#[cfg(target_os)]`，收编 8 个散落的 `static Mutex`。

- [ ] **3.1** ★ 新增 `platform/traits.rs`
  ```rust
  pub trait HitTest: Send + Sync {
      fn store_rects(&self, rects: &[Rect]);
      fn apply(&self) -> Result<(), AppError>;
      fn install(&self, app: &AppHandle) -> Result<(), AppError>;
  }
  pub trait AutoStart { fn enable(&self)->Result<(),AppError>; fn disable(&self)->Result<(),AppError>; fn is_enabled(&self)->Result<bool,AppError>; }
  pub trait WindowChrome { fn setup_pet_window(&self, w:&WebviewWindow)->Result<(),AppError>; fn setup_settings_window(&self, w:&WebviewWindow)->Result<(),AppError>; fn hide_pet_window(&self, w:&WebviewWindow)->Result<(),AppError>; }
  ```
- [ ] **3.2** `macos_pet.rs` → `platform/macos/hit_test.rs`（实现 `HitTest`）
  - ★ tick 内所有 `unwrap()` 清除（1.5 已在 Phase 1 做，此处确认）
  - ★ 拖拽状态机（DRAG_ARMED/ACTIVE/OFFSET/PRESS/DIR）移入 `state/` 统一管理
- [ ] **3.3** `platform/macos/activation.rs`：`set_activation_policy` 封装（`Regular` / `Accessory`）
- [ ] **3.4** `windows_pet.rs` → `platform/windows/region.rs`（`HitTest`）+ `dwm.rs`（`WindowChrome`）
- [ ] **3.5** `autostart.rs` → `platform/{macos,windows,linux}/autostart.rs`（实现 `AutoStart`）
  - 现有 `mod imp` 三段式结构可直接复用，仅需包一层 trait
- [ ] **3.6** `platform/mod.rs` 编译期分派
  ```rust
  #[cfg(target_os="macos")]   pub type PlatformHitTest = macos::hit_test::MacHitTest;
  #[cfg(target_os="windows")] pub type PlatformHitTest = windows::region::WinHitTest;
  #[cfg(not(any(...)))]       pub type PlatformHitTest = linux::NoopHitTest;
  ```
- [ ] **3.7** ★ 新增 `state/mod.rs`：`AppState` 统一持有，替代散落 static
  - 收编目标：`INTERACTIVE_RECTS` / `HIT_RECTS` / `RECTS_INITIALIZED` ×2 / `RECTS_DIRTY` / `LAST_MOUSE` / `LAST_FRAME_ORIGIN` / `DRAG_ACTIVE` / `DRAG_ARMED` / `DRAG_OFFSET` / `DRAG_PRESS` / `DRAG_DIR` / `PREV_OVER` / `NS_WINDOW_PTR` / `APP_HANDLE`
  - 全部通过 `app.manage(AppState::default())` 注册，command 用 `tauri::State<'_, AppState>` 获取
  - **收益**：状态可注入、可测试、无全局可变单例
- [ ] **3.8** ★ `main.rs` 拆分（441 行 → <120 行）
  - `app/builder.rs`：Builder 组装 + `invoke_handler` + 注册 state
  - `app/setup.rs`：setup 闭包（按平台分派安装/定位/显示）
  - `app/tray.rs`：`build_tray_menu` / `rebuild_tray_menu` / 菜单事件 match
  - `app/window_events.rs`：settings 关闭隐藏 + 位置持久化
  - `main.rs` 只剩 `fn main() { petbuddy_lib::run() }`
- [ ] **3.9** ★ 新增 `commands/` 薄层
  - `commands/mod.rs` 集中所有命令名常量 + `generate_handler!`
  - 每个 command 函数体 ≤ 20 行：反序列化 → 校验 → 调 `domain`/`platform`/`infra` → 包装错误
- [x] **3.10** 新增 `get_platform()` command，返回 `"macos" | "windows" | "linux"`
  - 前端 `shared/platform/index.ts` 改从此获取（Phase 5.2 对接）

**✅ Phase 3 验收**：`grep -rn "cfg(target_os" src/ | grep -v platform/` **结果为空**；`main.rs` < 120 行；功能基线 A10/A11/D4 在 macOS 与 Windows 双端实测通过。

---

### Phase 4 Rust：基础设施与安全加固

- [ ] **4.1** ★ `infra/http_client.rs`
  - `OnceLock<Client>` + `timeout(15s)` + `connect_timeout(5s)` + 统一 UA
  - 提供 trait `HttpClient`（便于测试注入 mock，覆盖 `browse_online_pets` / `download_online_pet`）
- [ ] **4.2** ★ `infra/http_server.rs` 加固（承接 1.3）
  - Host 头白名单 `127.0.0.1` / `localhost`（挡 DNS-rebinding 与浏览器 CSRF）
  - `Content-Length` 上限 8KB → 413
  - 单次连接总超时 10s（非仅 read timeout）
  - 并发连接信号量（上限 32）
  - 每连接线程改为**读取上限内的固定缓冲**，杜绝无限增长
  - bind 失败 → `app.emit("notify-server-error")`，前端提示端口被占用
  - 🧪 集成测试 `tests/http_server.rs`：真端口 + 真 socket，覆盖上述全部路径
- [ ] **4.3** ★ `infra/storage.rs`
  - `trait PetStorage { fn pets_root(&self) -> PathBuf; fn read/write/remove ... }`
  - 实现 `AppDataStorage`；测试用 `TempDirStorage`
  - 🧪 集成测试 `tests/pet_storage.rs`：导入 → 落盘 → list → delete → 断言文件消失
- [ ] **4.4** 🧪 `tests/zip_slip.rs`：构造含 `../evil.txt`、绝对路径 `/etc/passwd`、符号链接条目的 zip → 断言全部未逃逸出 `pets_root`
- [ ] **4.5** `infra/archive.rs`：zip 解压（纯 IO），解压总大小上限（防 zip bomb，如 200MB / 1000 条目）
- [ ] **4.6** ★ 安全配置收敛
  - `tauri.conf.json` 配置 CSP：`"csp": "default-src 'self'; img-src 'self' data: https://codexpet.top; connect-src 'self' ipc: http://ipc.localhost"`
  - `capabilities/default.json`：把 `core:default` 拆成最小权限集（仅保留 window/show/hide/set-size/set-position/start-dragging + event 默认）
  - `broadcast_event` 加**事件名白名单**（只允许 `pet-*` / `notify-push` / `state-changed`）
- [ ] **4.7** 移除 `notify_server` 中的 `println!`，改用统一 `log` 宏（`tauri::log` 或 `tracing`）

**✅ Phase 4 验收**：`tests/` 三个集成测试全绿；安全扫描（自测）：外部网页无法 POST 通知、超大 body 被拒、zip slip 被拦。

---

### Phase 5 前端：基础设施

- [x] **5.1** ★★ `shared/config/constants.ts`（唯一真源）
  ```ts
  export const FRAME = { WIDTH: 192, HEIGHT: 208, COLS: 8 } as const
  export const SCALE = { MIN: 0.5, MAX: 1.3, STEP: 0.05, DEFAULT: 0.7 } as const
  export const WINDOW = {
    BASE_WIDTH: 320,
    PAD: 24,
    EDGE_GAP_X: 24,
    EDGE_GAP_Y_MAC: 75,
    EDGE_GAP_Y_WIN: 64,
  } as const
  export const NOTIFY = { PORT: 8756, MAX_LEN: 120, DEFAULT_DURATION_MS: 4000 } as const
  export const BUBBLE = {
    MAX_WIDTH: 300,
    SHADOW_PAD: 28,
    PET_SHADOW_PAD: 16,
    ENTER_MS: 300,
    LEAVE_MS: 220,
    SETTLE_MS: 200,
  } as const
  export const DRAG = { THRESHOLD_PX: 6, SETTINGS_THRESHOLD_PX: 5, MOVED_DEBOUNCE_MS: 180 } as const
  export const TIMING = { RANDOM_MIN_MS: 6000, RANDOM_MAX_MS: 15000, CHAT_MS: 3000 } as const
  ```
  - ⚠️ **要求**：后续由 Rust `domain` 通过 ts-rs 生成同值文件，CI 校验两份一致（Phase 9.5）
- [x] **5.2** ★ `shared/platform/index.ts`
  - `let platform: Platform | null = null`；`initPlatform()` 在 `App.vue` onMounted 调 `get_platform()`
  - `getPlatform()` 同步返回（已初始化后）；未初始化时 fallback 到 `isTauri ? 'unknown' : 'web'`
  - **删除 `PetHost.vue:11-13` 的 `navigator.platform` 嗅探**
- [x] **5.3** ★ `shared/ipc/client.ts`
  - `invokeTyped<T>(cmd, args?, opts?: {timeoutMs})`：统一 catch → 抛 `AppError`（含 code）
  - 非 Tauri 环境返回 `Result` 风格的 fallback（可配置）
  - 集中日志（带命令名）
- [x] **5.4** ★ `shared/ipc/events.ts`
  - `emitCrossWindow(event, payload)`：自动附加 `source: windowId`
  - `onCrossWindow(event, handler)`：自动跳过 `source === myWindowId`（**替代现有"值相等防回环"**）
  - 内部维护 listener 集合，提供 `disposeAll()`
- [x] **5.5** `shared/errors/AppError.ts` + `messages.ts`
  - `AppError { code, message, cause? }`；`messages.ts` 做 `ErrorCode → 中文文案` 映射（与 Rust `ErrorCode` 一一对应）
- [x] **5.6** ★ `src/test/mocks/tauri.ts`
  - mock `@tauri-apps/api/core`（invoke）、`@tauri-apps/api/event`（listen/emit）、`@tauri-apps/api/window`
  - 提供 `mockInvoke(map)` / `emitEvent(name, payload)` / `invocationLog()` 断言工具
  - **这是让全部前端逻辑可在 jsdom 下测试的关键**
- [x] **5.7** `shared/styles/tokens.css`：把 `style.css` 的 `:root` 变量整体迁入，作为**设计令牌唯一真源**
  - 补充缺失令牌：`--radius-bubble: 14px`、`--radius-window: 6px`、`--radius-modal: 8px`、`--shadow-bubble`
  - ⚠️ **解决 6px/8px 矛盾**：明确 `settings 窗口/蒙版 = --radius-window(6px)`，`弹窗卡片 = --radius-modal(8px)`，注释与代码统一

**✅ Phase 5 验收**：`grep -rn "navigator.platform\|navigator.userAgent" src/` **为空**；`shared/` 层零 Vue 依赖；mocks 可用且至少支撑 10 个组件测试。

---

### Phase 6 前端：纯逻辑下沉（`model/`）

**目标**：把 `PetHost.vue` 里的状态机、队列、几何计算全部抽成**不依赖 Vue 的纯函数**，然后 100% 单测。

- [x] **6.1** ★ `features/pet/model/frame.ts`
  - `seqFor(pet, state) -> FrameSeq | null`（从 `SpritePet.vue:40`）
  - `frameBounds(imgW, imgH, frame) -> {rows, cols}`（从 `SpritePet.vue:65-66`）
  - `isFrameInBounds(row, col, bounds) -> boolean`
  - 🧪 补：越界返回 false（不清空画布）；9 行图访问 row 10 → 越界；非标准 cols
- [x] **6.2** ★★ `features/pet/model/actionScheduler.ts`（纯 reducer）
  ```ts
  type ActionState = { current: string; isPlaying: boolean }
  type ActionEvent =
    | { type: 'play'; name; durationMs }
    | { type: 'finish' }
    | { type: 'randomTick'; pool; rng }
    | { type: 'forceIdle' }
  function reduce(state: ActionState, ev: ActionEvent): ActionState
  ```
  - 从 `PetHost.vue` 抽出：`playAction` / `playRandomAction` / `scheduleRandomAction` / `RANDOM_POOL` / hover 的 waiting 逻辑
  - **修复现有隐患**：三个 timer（action/random/chat）互相 clear 的风险由状态机统一消除
  - 🧪 补：talk 优先级最高；随机池过滤不存在的动作；`finish` 后回 idle；rng 注入使测试确定性
- [x] **6.3** ★ `features/pet/model/geometry.ts`
  - `computeHitRects({petRect, bubbleRect, scale, tokens}) -> Rect[]`
  - `padRect(rect, pad) -> Rect`（从 `PetHost.vue:214-215`，padding 由 tokens 推导而非硬编码）
  - ⚠️ **规则**：`petRect` 不存在但 `bubbleRect` 存在时返回 `[]`（保持现有"整窗可交互"语义）
  - 🧪 补：阴影 padding 随 scale 线性；空宠物时的降级分支（回归启动竞态 bug）
- [x] **6.4** ★ `features/notify/model/notifyQueue.ts`
  ```ts
  type QueueState = { queue: NotifyItem[]; current: NotifyItem | null }
  function enqueue(state, item): QueueState
  function next(state): QueueState // 出队
  function durationOf(item): number // item.duration ?? DEFAULT
  ```
  - 从 `PetHost.vue:99-138`，**修复 P0-4**
  - 🧪 补：FIFO 顺序；`action` 非空时先播动作；空 text 不入队；duration 透传
- [x] **6.5** `features/gallery/model/filter.ts`（从 `PetSettings.vue:248-257`）
  - 🧪 补：名字/作者/分类三字段、大小写不敏感、空关键词返回全部、无匹配返回空
- [x] **6.6** `features/pet/model/dialogues.ts`：迁移 `src/pets/dialogues.ts`
  - 增加 `pickDialogue(petId, action, rng)` 纯函数（从 `PetHost.vue:76-87`）
  - 结构预留 i18n key（暂只填 zh）
  - 🧪 补：内置宠物命中专属台词；外部宠物走通用；动作无台词 → 空串
- [x] **6.7** `shared/utils/base64.ts`：迁移 `arrayBufferToBase64`（从 `PetSettings.vue:308`）
  - 🧪 补：>0x8000 分块正确性；空 buffer；单字节

**✅ Phase 6 验收**：`features/**/model/**` 与 `shared/**` 覆盖率 ≥90%；这些文件**零 `import { ref } from 'vue'`**（lint 规则断言）；功能基线 A4/A5/A6/A7/B1~B4 全绿。

---

### Phase 7 前端：组件拆分

- [ ] **7.1** ★★ `PetHost.vue`（763 行）拆分
  - `windows/PetWindow.vue`（<200 行）：只做「组合子 composable + 渲染」，无业务逻辑
  - `composables/usePetActions.ts`：包装 `actionScheduler`（对接 timer）
  - `composables/useNotifyQueue.ts`：包装 `notifyQueue`（对接 timer）
  - `composables/usePetDrag.ts`：★ 平台差异在此隔离
    ```ts
    // 内部按 platform 选择策略
    const strategy = platform === 'macos' ? nativeDragStrategy : domDragStrategy
    ```
    - `nativeDragStrategy`：监听 `pet-drag-start/drag/drag-end`
    - `domDragStrategy`：mousedown/mousemove 阈值 + `startDragging` + `onWindowMoved` 兜底
  - `composables/useHitRects.ts`：气泡/宠物/scale/visible 变化 → `reportInteractiveRectsSettled`
    - 保留现有 `leavingBubbleRect` 缓存机制与 200ms 兜底时序（**这是踩过坑的逻辑，原样迁移 + 补注释**）
  - `components/PetBubble.vue`：气泡 UI + `::after` 箭头 + Transition 钩子
  - `components/PetStage.vue`：宠物容器 + 事件绑定
  - 🧪 补组件测试：`PetBubble` 的 duration / 排队 / 离场缓存矩形
- [ ] **7.2** ★★ `PetSettings.vue`（1635 行）拆分
  - `windows/SettingsWindow.vue`（<150 行）：布局编排 + 窗口拖拽
  - `components/SettingsShell.vue`：header + 品牌 + 关闭
  - `components/PetPreviewPanel.vue`：预览 + 名字 + 版本号
  - `components/ScaleSlider.vue`：自绘滑块（从 `PetSettings.vue:131-158`）
  - `components/ToggleField.vue`：`显示宠物` 开关
  - `components/PetListPanel.vue`：列表 + 编辑/删除二次确认
  - `components/NotifyTestDialog.vue`：测试通知 + curl 复制 + 冷却
  - `components/PetEditDialog.vue`：编辑宠物信息
  - `gallery/components/OnlineGalleryDialog.vue` + `GalleryCard.vue`
  - 🧪 补组件测试：`ScaleSlider` 拖动映射与 0.05 步进；`PetListPanel` 二次点击确认；`OnlineGalleryDialog` 四态
- [ ] **7.3** ★ 样式治理
  - 所有组件 `scoped` 样式中的硬编码色值替换为 tokens（`#e5484d` → `--danger`；`rgba(31,39,51,x)` → `--ink-alpha-x`）
  - 圆角统一走 `--radius-*`
  - 阴影统一走 `--shadow-*`
  - ⚠️ **注意**：改阴影必须同步 `BUBBLE.SHADOW_PAD`，并在 Windows 真机验证裁剪不切阴影（A11）
- [ ] **7.4** 模板格式整理：`PetSettings.vue:603-648` 的缩进错乱修正
- [ ] **7.5** `composables/useTauriEvent.ts` 全量替换手写 `onEvent`（承接 1.6）
- [ ] **7.6** 删除 `src/pets/` 旧目录（内容已迁至 `features/pet/model/`），更新 manifest 引用

**✅ Phase 7 验收**：`windows/*.vue` 均 < 200 行；`grep -rn "#[0-9a-fA-F]\{3,6\}" src/**/*.vue` 仅剩 tokens.css 与 SVG 渐变；功能基线 C 全量 12 项通过。

---

### Phase 8 状态与持久化统一

- [ ] **8.1** ★ Rust 新增 `state_store`：`get_state()` / `set_state(patch)` command
  - 持久化到 `app_data_dir/petbuddy_state.json`（字段：`currentPetId` / `scale` / `visible` / `settingsWindowPos`）
  - 写入后 `app.emit("state-changed", fullState)` 广播
- [ ] **8.2** ★ 前端 store 改为**内存镜像 + 订阅**
  - `usePetStore` / `useSettingsStore` 启动时 `await get_state()` 填充
  - 所有变更走 `set_state`，由 `state-changed` 事件回灌内存（**天然解决跨窗口同步，无需防回环**）
  - ⚠️ **移除 localStorage 作为跨窗口真源的依赖**（消除 §架构债 #2 的平台不确定性）
- [ ] **8.3** settings 窗口位置纳入同一 state（删除 `petbuddy_settings_window.json` 独立文件）
- [ ] **8.4** 统一状态迁移：首次启动时把旧 localStorage 值迁移到 state 文件（**保证老用户升级不丢配置**）
- [ ] **8.5** 🧪 补：state 序列化/反序列化 roundtrip；跨窗口同步（mock emit 后断言两个 store 实例一致）；旧值迁移

**✅ Phase 8 验收**：功能基线 C11（三端同步）双端实测；老版本配置升级后宠物选择/缩放/显隐保持不变。

---

### Phase 9 自动化测试体系

> 详细用例清单见 **[TEST_PLAN.md](./TEST_PLAN.md)**。此处仅列工程任务。

- [ ] **9.1** Rust 单测：`domain/` 全覆盖（目标 line ≥90%）
- [ ] **9.2** Rust 集成测试：`tests/http_server.rs`、`tests/pet_storage.rs`、`tests/zip_slip.rs`
- [ ] **9.3** 前端单测（vitest）：`model/` + `shared/`（目标 ≥90%）
- [ ] **9.4** 前端组件测试（@vue/test-utils + jsdom）：8 个核心组件
- [ ] **9.5** ★ 契约测试
  - Rust 侧引入 `ts-rs`，为 `PetDefJson` / `NotifyPayload` / `AppError` / `Platform` 生成 `src/bindings/*.ts`
  - 前端从生成文件 import 类型
  - CI job `contract`：`cargo test export_bindings && git diff --exit-code src/bindings/`（**漂移即失败**）
- [ ] **9.6** E2E（tauri-driver + WebdriverIO）
  - `.github/workflows/e2e.yml`，macOS runner
  - 冒烟用例对应功能基线 A1/A3/A5/A8/B1/C3/C4/C6
- [ ] **9.7** 覆盖率门禁
  - `vitest --coverage` 配置 thresholds（global 60%，`model/`+`shared/` 90%）
  - `cargo llvm-cov` 或 `cargo-tarpaulin` 输出 lcov，上传 Codecov
  - CI 覆盖率下降 >2% 即 fail
- [ ] **9.8** 手动测试清单文档化（`docs/refactor/MANUAL_TEST.md`）
  - **必须人工验证项**（自动化无法覆盖）：macOS 穿透手感、Windows 裁剪不切阴影、拖拽跟手度、多显示器 DPI 切换

**✅ Phase 9 验收**：`npm run verify` + `cargo test` 全绿；覆盖率达标；CI 四个 job（quality / contract / e2e / build）全通过。

---

### Phase 10 发布工程与文档治理

- [ ] **10.1** ★ 版本号单一真源
  - 现状：`package.json` / `tauri.conf.json` / `Cargo.toml` 三处 + `build.sh` 与 CI 各写一遍同步逻辑（**两份脚本重复**）
  - 方案：`scripts/bump-version.mjs` 单一实现，`build.sh` 与 `build-windows.yml` 都调用它
- [ ] **10.2** ★ CI 重编排
  ```
  quality (ubuntu)   ─┬─► contract (ubuntu) ─┬─► build (macos-14 / windows-latest / ubuntu-22.04)
  [required]          └─► e2e (macos-14) ────┘        └─► release (on tag v*)
  ```
  - `quality` 与 `contract` 设为 **required status checks**（禁止未过门禁合并）
  - `build-windows.yml` 现有发布流程保留，仅复用 `bump-version.mjs`
- [ ] **10.3** 大文件治理
  - `pet/*.zip`（38MB）迁 Git LFS，或移入 Release assets 并在 README 改下载链接
  - `git filter-repo` 清理历史（⚠️ 需协调，会改写历史）
  - `website/.DS_Store` 删除 + `.gitignore` 补 `**/.DS_Store`
- [ ] **10.4** ★ 文档漂移防护
  - `README.md` 中的**技术事实**（自启机制、轮询间隔、Info.plist 方式、缩放范围）改为从代码生成的片段，或加 CI 校验脚本
  - 新增 `CHANGELOG.md`，采用 Conventional Commits + 自动生成
  - `commitlint` + `husky` pre-commit 跑 `lint-staged`（eslint --fix + prettier + cargo fmt）
- [ ] **10.5** 新增 `CONTRIBUTING.md`：分支策略、PR 模板、本地验证命令、架构决策记录（ADR）规范
- [ ] **10.6** 建立 `docs/adr/` 记录关键决策（至少：`0001-双窗口状态同步方案`、`0002-跨平台穿透抽象`、`0003-纯领域层与集成测试策略`）

**✅ Phase 10 验收**：一次完整的 tag → CI → Release 流程跑通；`README` 描述与代码一致；新成员按 `CONTRIBUTING.md` 可 30 分钟内跑起全量验证。

---

## 4. 工作量估算与排期

| Phase    | 内容                    | 人日          | 可并行             | 前置                              |
| -------- | ----------------------- | ------------- | ------------------ | --------------------------------- |
| 0        | 工具链地基              | 1.5           | —                  | —                                 |
| 1        | 止血与一致性            | 4             | 部分               | 0                                 |
| 2        | Rust 抽纯领域层         | 5             | 否（关键路径）     | 0                                 |
| 3        | Rust 平台抽象与状态集中 | 6             | 部分               | 2                                 |
| 4        | Rust 基础设施与安全     | 4             | 是（与 5 并行）    | 2                                 |
| 5        | 前端基础设施            | 4             | 是（与 4 并行）    | 0                                 |
| 6        | 前端纯逻辑下沉          | 5             | 否                 | 5                                 |
| 7        | 前端组件拆分            | 6             | 部分               | 6                                 |
| 8        | 状态与持久化统一        | 4             | 是（与 7 并行）    | 5, 3                              |
| 9        | 自动化测试体系          | 8             | 部分（7/8 后集中） | 2~8                               |
| 10       | 发布工程与文档          | 3             | 是                 | 0                                 |
| **合计** |                         | **≈ 50 人日** |                    | **≈ 10 周（1 人）/ 5 周（2 人）** |

**建议排期（2 人协作）**：

```
W1        : Phase 0 + Phase 1（止血，独立可发版）
W2        : Phase 2（Rust 纯逻辑，A 负责）  ‖  Phase 5（前端基建，B 负责）
W3        : Phase 3 + 4（A）                ‖  Phase 6（B）
W4        : Phase 7（B）                    ‖  Phase 8（A）
W5        : Phase 9 集中补测 + Phase 10
```

---

## 5. 风险登记册与缓解措施

| ID   | 风险                                                          | 影响               | 概率 | 缓解措施                                                                                                                                            |
| ---- | ------------------------------------------------------------- | ------------------ | ---- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| RK1  | **macOS NSTimer + objc2 hook 重构引入崩溃**                   | 高（App 无法启动） | 中   | Phase 3 前先完成 P0-5（catch_unwind + 去 unwrap）；重构后立即在 macOS 12/13/14 三版本实测；保留 `class_replaceMethod` 的降级分支与自检日志          |
| RK2  | **Windows SetWindowRgn 时序被破坏导致气泡被裁**               | 高（核心卖点失效） | 中   | `useHitRects` 迁移时**逐字保留**现有的 `leavingBubbleRect` + 200ms 兜底逻辑与注释；Phase 7 结束后在 Windows 10/11 × 100%/125%/150% DPI 三档实测 A11 |
| RK3  | **状态持久化从 localStorage 切到 Rust state，老用户配置丢失** | 中                 | 中   | Phase 8.4 强制实现迁移逻辑；测试覆盖"旧 localStorage 存在 → 升级 → 配置保持"                                                                        |
| RK4  | **`domain/` 抽取时意外改变浮点舍入，窗口尺寸差 1px**          | 中（视觉错位）     | 中   | Phase 2.3 的黄金值快照测试（scale=0.5/0.7/1.0/1.3）在抽取**前**先写好并跑通，抽取后断言不变                                                         |
| RK5  | **组件拆分破坏 Vue 响应式/Timer 时序**                        | 高（动画错乱）     | 中   | 拆分前先抽 model 纯函数并补测（Phase 6）；拆分时严格"先复制再删除"，每删一段跑一次功能基线                                                          |
| RK6  | **`lib.rs` 改造导致 Tauri 打包失败**                          | 高（无法发布）     | 低   | 采用 Tauri 官方模板的 `petbuddy_lib` 命名与 `crate-type` 组合；Phase 2.1 完成后**立刻跑一次完整 `npm run tauri build`** 验证产物                    |
| RK7  | **zip 解压行为变更导致已有宠物包导入失败**                    | 中                 | 低   | `pet/` 下 9 个真实 zip 全部纳入集成测试 fixture，逐个断言导入成功                                                                                   |
| RK8  | **ESLint 依赖规则误伤，阻塞开发**                             | 低                 | 高   | 规则先设为 `warn` 观察 2 周，再转 `error`；提供 `// eslint-disable-next-line` 的白名单登记机制                                                      |
| RK9  | **E2E 在 CI 上不稳定（窗口/时机敏感）**                       | 中（CI 噪声）      | 高   | E2E 只放 6 条最稳的冒烟；失败自动重试 2 次；E2E **不设为 required**，仅作信号                                                                       |
| RK10 | **工作量超预期，重构烂尾**                                    | 高                 | 中   | 严格按 Phase 推进，每个 Phase 结束即可发版；若中途必须停，**停在 Phase 1 / 2 / 6 末尾**都是净收益状态                                               |

---

## 6. 回滚策略

| 层级                         | 机制                                                                                                                                                  |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| **提交粒度**                 | 每个任务项一个 commit，消息格式 `refactor(phase2): 抽取 domain/layout`；任何一项都可单独 `git revert`                                                 |
| **Phase 粒度**               | 每 Phase 开始前打 `pre-phase-N` tag；Phase 验收失败 → `git reset --hard pre-phase-N`                                                                  |
| **格式化隔离**               | Phase 0.10 的格式化是独立 commit 且登记进 `.git-blame-ignore-revs`，可单独 revert 而不影响后续逻辑改动                                                |
| **双轨并行（高风险项专用）** | 对 RK1 / RK2 两项，采用「新旧共存 + feature flag」：新增实现与旧实现同时存在，通过环境变量 `PETBUDDY_NEW_HITTEST=1` 切换；验证 2 周无问题后再删旧实现 |
| **发布回滚**                 | CI 每次 tag 都产出完整 Release 产物；出问题直接回滚到上一个 tag 的 Release，无需重新构建                                                              |

---

## 7. 完成的定义（DoD）

整个重构完成，必须同时满足：

### 7.1 结构

- [ ] `main.rs` < 120 行；`windows/*.vue` 均 < 200 行；无单文件 > 400 行（样式文件除外）
- [ ] `grep -rn "cfg(target_os" src-tauri/src/ | grep -v "src-tauri/src/platform/"` 为空
- [ ] `grep -rn "navigator.platform\|navigator.userAgent" src/` 为空
- [ ] `domain/` 与 `features/**/model/` 中零 IO、零平台 API、零 Vue 依赖（由 lint 规则强制）

### 7.2 质量

- [ ] `npm run verify` 全绿（typecheck + lint + format + 前端测试 + Rust 测试）
- [ ] `cargo clippy --all-targets -- -D warnings` 零告警
- [ ] 覆盖率：Rust `domain/` ≥90%；前端 `model/`+`shared/` ≥90%；整体各 ≥60%
- [ ] 契约测试通过（Rust 生成的 TS 类型与仓库内一致）

### 7.3 功能

- [ ] 功能基线 A/B/C/D/E **共 40 项全部勾选通过**
- [ ] macOS（12/13/14）与 Windows（10/11，100%/125%/150% DPI）双端实测
- [ ] 老版本配置升级路径验证通过

### 7.4 工程

- [ ] CI 四个 job（quality / contract / e2e / build）通过，quality + contract 为 required
- [ ] `README.md` 技术描述与代码一致（有 CI 校验或生成机制）
- [ ] `docs/ARCHITECTURE.md` + `CONTRIBUTING.md` + 至少 3 篇 ADR 就位
- [ ] 一次完整的 tag → CI → Release 流程跑通

---

## 附：立即可执行的第一步

若需立刻开工，建议按此顺序送出前 3 个 PR（半天内可完成，且零风险）：

1. **PR-1（Phase 0.1~0.9）**：加 `rustfmt.toml` / `clippy.toml` / `eslint.config.js` / `.prettierrc` / `vitest.config.ts` / `quality.yml`，安装测试依赖。**不改任何业务代码。**
2. **PR-2（Phase 1.11~1.13）**：修 `气ß泡` 乱码、删 `--pet-w` 死代码、删 `file_name` 死参数、修 `main.rs` 注释与 README 三处漂移。**纯文本改动。**
3. **PR-3（Phase 1.4 + 1.8 + 1.9）**：`duration` 生效 + 缩放不闪 + 切宠不残影，各带一条回归测试。**三个独立小 bug 修复。**

这 3 个 PR 落地后即建立"改代码 → 有测试 → 有门禁"的正循环，后续大改才有安全网。
