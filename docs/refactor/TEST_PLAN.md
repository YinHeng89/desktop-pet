# PetBuddy 自动化测试方案

> 配套文档：[REFACTOR_PLAN.md](./REFACTOR_PLAN.md)（Phase 9 是本文档的落地阶段）
>
> **核心思路**：不做"为了覆盖率而写的测试"，而是**围绕已实际发生过的 bug 和无法靠肉眼回归的平台时序逻辑**来建测试网。
> 本项目历史上出现过的每一类 bug（VP8 位宽写错、SetWindowRgn 时序、hide 顺序反了、气泡离场矩形消失、
> 拖拽超时竞态……）都必须有一条对应的**回归用例**，否则重构后必然重犯。

---

## 目录

- [1. 测试金字塔与选型](#1-测试金字塔与选型)
- [2. L1 Rust 单元测试（`domain/`）](#2-l1-rust-单元测试domain)
- [3. L2 Rust 集成测试（`tests/`）](#3-l2-rust-集成测试tests)
- [4. L3 前端单元测试（`model/` + `shared/`）](#4-l3-前端单元测试model--shared)
- [5. L4 前端组件测试（@vue/test-utils）](#5-l4-前端组件测试vue-test-utils)
- [6. L5 契约测试（Rust ↔ TS）](#6-l5-契约测试rust--ts)
- [7. L6 E2E（tauri-driver）](#7-l6-e2etauri-driver)
- [8. 手动测试清单（自动化不可覆盖）](#8-手动测试清单自动化不可覆盖)
- [9. 覆盖率门禁](#9-覆盖率门禁)
- [10. CI 编排](#10-ci-编排)
- [11. 测试数据与 Fixture 清单](#11-测试数据与-fixture-清单)

---

## 1. 测试金字塔与选型

```
                 ┌──────────────┐
                 │  L6 E2E      │  6 条冒烟，非 required，仅信号
                 │  tauri-driver│  ~5 min
                 ├──────────────┤
                 │  L5 契约      │  ts-rs 生成 + git diff 校验
                 │  Rust ↔ TS   │  ~10 s
                 ├──────────────┤
              ┌──┴──────────────┴──┐
              │  L4 组件测试        │  8 组件，jsdom
              │  @vue/test-utils   │  ~30 s
              ├────────────────────┤
              │  L3 前端单测        │  model/ + shared/，纯函数
              │  vitest            │  ~5 s   ★ 性价比最高
              ├────────────────────┤
              │  L2 Rust 集成       │  真端口 / 真 tempdir / 真 zip
              │  cargo test        │  ~20 s
              ├────────────────────┤
              │  L1 Rust 单测       │  domain/ 纯函数
              │  cargo test        │  ~3 s   ★ 性价比最高
              └────────────────────┘
```

| 层           | 工具                                   | 目标覆盖                | 运行时机                  | 是否 required   |
| ------------ | -------------------------------------- | ----------------------- | ------------------------- | --------------- |
| L1 Rust 单测 | `cargo test`（内建）                   | `domain/` line ≥90%     | 每次 push                 | ✅              |
| L2 Rust 集成 | `cargo test --test *`                  | 关键 IO 路径 100%       | 每次 push                 | ✅              |
| L3 前端单测  | `vitest`                               | `model/`+`shared/` ≥90% | 每次 push                 | ✅              |
| L4 组件测试  | `vitest` + `@vue/test-utils` + `jsdom` | 8 个核心组件主路径      | 每次 push                 | ✅              |
| L5 契约      | `ts-rs` + `git diff --exit-code`       | 所有跨语言 DTO          | 每次 push                 | ✅              |
| L6 E2E       | `tauri-driver` + WebdriverIO           | 6 条冒烟                | 每次 push（macOS runner） | ❌（重试 2 次） |

**选型理由**：

- **不引入 Playwright**：Tauri 是原生窗口，Playwright 无法驱动；官方方案是 `tauri-driver` + WebdriverIO。
- **前端不引入 Cypress**：Vitest 与 Vite 同源，配置成本近乎为零，且本项目 90% 的逻辑是纯函数，jsdom 足够。
- **Rust 不引入 mockall**：`domain/` 层全部设计成纯函数（输入 `&[u8]`/`&str` → 输出 `Result`），天然可测；
  仅 `infra::http_client` 与 `infra::storage` 需要 trait + 手写 mock（各 20 行，不值得引框架）。

---

## 2. L1 Rust 单元测试（`domain/`）

> 位置：`src-tauri/src/domain/**`（`#[cfg(test)] mod tests`，与源码同文件）
> 前置：REFACTOR_PLAN Phase 2

### 2.1 `domain/geometry.rs` ✅ 已有 3 个，扩充到 12 个

**现有**：`clamp_scale_bounds` / `point_in_rects_basic` / `rects_to_logical_physical_scales`

**新增**：

- [ ] `clamp_scale_nan_and_inf` — `NaN` 与 `±inf` 不应 panic，返回确定值
- [ ] `clamp_scale_precision` — `MIN + 1e-9` / `MAX - 1e-9` 边界
- [ ] `point_in_rects_empty_and_gap` — 空列表任何点不命中；两矩形间隙不命中（已有部分，拆细）
- [ ] `point_in_rects_corner_exact` — 四角精确命中（`(x,y)`、`(x+w,y+h)` 含边界）
- [ ] `point_in_rects_overlapping` — 重叠矩形命中应为 true
- [ ] `point_in_rects_negative_size` — `w<0` 或 `h<0` 的畸形矩形不应命中
- [ ] `rects_to_logical_physical_zero_scale` — `scale=0` 不 panic
- [ ] `rects_to_logical_physical_empty` — 空输入返回空
- [ ] `rects_to_logical_physical_rounding` — 断言舍入方式（`0.5` 的处理一致）

### 2.2 `domain/layout.rs` ★ 新增（窗口尺寸核心）

- [ ] `pet_window_size_golden_0_5` — `(w, h)` 黄金值快照
- [ ] `pet_window_size_golden_0_7` — **必须等于 `248 × 295`**（README 已记载，防漂移）
- [ ] `pet_window_size_golden_1_0` — `344 × 404`
- [ ] `pet_window_size_golden_1_3`
- [ ] `pet_window_size_clamps_scale` — 传入 `0.1` / `5.0` 结果等于 `0.5` / `1.3` 的结果
- [ ] `window_width_never_below_pet_width` — 任何 scale 下 `w >= pet_w`（断言 `max()` 语义未丢）
- [ ] `anchor_bottom_right_first_resize` — 已知旧 pos/size，断言新左上角使右下角不变
- [ ] `anchor_bottom_right_repeated` — ★ 连续 10 次 resize，右下角累计漂移 < 1px
      （回归风险 RK4：浮点舍入导致的"每帧跳一下"，见 `main.rs:97-99` 注释）
- [ ] `anchor_bottom_right_scale_factor_2` — scale_factor=2 时物理像素换算正确

### 2.3 `domain/pet/codec.rs` ★ 新增（历史 bug 高发区）

**`webp_dimensions`**（9 个）：

- [ ] `webp_vp8_lossy_standard` — 1920×2080 的 VP8 lossy 样本
- [ ] `webp_vp8l_lossless_standard` — VP8L 样本
- [ ] `webp_vp8x_extended_standard` — VP8X 样本
- [ ] `webp_vp8_large_width_regression` — ★ **宽度 ≥ 16384**
      （回归 `pet_import.rs:82-84` 记录的历史 bug：误用 VP8L 的 14 位掩码 `& 0x3fff` 丢弃高位）
- [ ] `webp_vp8l_max_14bit` — 宽/高均为 `16383`（14 位最大值）的 VP8L
- [ ] `webp_vp8x_max_24bit` — VP8X 的 24 位 width-1 边界
- [ ] `webp_rejects_non_webp` — PNG/JPEG 头 → `None`
- [ ] `webp_rejects_truncated` — 截断到 12 字节 / 20 字节 → `None`
- [ ] `webp_rejects_empty` — 空 slice → `None`

**`base64`**（6 个）：

- [ ] `base64_roundtrip_all_lengths` — 长度 0..=64 全覆盖 roundtrip
- [ ] `base64_encode_padding_1_byte` — 尾部 1 字节 → `xx==`
- [ ] `base64_encode_padding_2_bytes` — 尾部 2 字节 → `xxx=`
- [ ] `base64_decode_rejects_invalid_char` — 含 `!` → `Err`
- [ ] `base64_decode_tolerates_whitespace` — 含 `\n` `\r` 空格 `=`
      （⚠️ 已知限制：tab 会报错，若决定修复则改为通过）
- [ ] `base64_roundtrip_large` — 2MB 随机数据 roundtrip 一致

### 2.4 `domain/pet/validator.rs` ★ 新增

**`is_valid_pet_id`**（12 个，正反各 6）：

- [ ] 正例：`miku` / `Seedy` / `a-b_C9` / `pet123` / `A` / `my_pet-2`
- [ ] 反例：空串 / `../etc` / `a/b` / `宠物` / `a b` / `a.b`
      （回归：`pet_import.rs` 三处重复的正则，抽取时必须行为一致）

**`clamp_seq`**（6 个）：

- [ ] `clamp_seq_row_in_bounds_unchanged`
- [ ] `clamp_seq_row_out_of_bounds_returns_none` — ★ 越界动作被移除（回归 E2）
- [ ] `clamp_seq_zero_count_uses_default` — `count=0` → `FRAME_COLS`
- [ ] `clamp_seq_count_exceeds_cols_truncated` — `count=20, cols=8` → 8
- [ ] `clamp_seq_zero_fps_defaults_to_8` — `fps=0` → 8（防动画卡死）
- [ ] `clamp_seq_count_becomes_zero_returns_none`

**`safe_join`**（5 个）：

- [ ] `safe_join_simple` / `safe_join_rejects_dotdot` / `safe_join_rejects_absolute`
      / `safe_join_rejects_empty` / `safe_join_rejects_symlink_ish`

### 2.5 `domain/pet/model.rs` ★ 新增

- [ ] `build_pet_def_standard_11_rows` — 11 行图，默认 actions 全部保留
- [ ] `build_pet_def_9_rows_removes_out_of_range` — ★ 9 行图（miku 场景）：`look`(row 9) / `working`(row 7) 中越界的被移除
      （回归 E2，对应 `pet_import.rs:213-216` 注释）
- [ ] `build_pet_def_unparsable_falls_back_11_rows` — 非 webp 字节 → 按 11 行处理且不打误报警告
- [ ] `build_pet_def_per_pet_frame` — ★ 非 192×208 帧尺寸（如 256×256、6 列）正确计算 cols/rows
      （对应 P0-1）
- [ ] `build_pet_def_explicit_seq_overrides_default` — `pet.json` 提供 idle/talk/actions 时覆盖默认
- [ ] `build_pet_def_empty_id_from_json` — `raw.id` 为空时的处理
- [ ] `build_pet_def_display_name_fallback` — 无 `displayName` → 用 `id`
- [ ] `build_pet_def_spritesheet_is_data_url` — 断言前缀 `data:image/webp;base64,`

### 2.6 `domain/notify/http_request.rs` ★ 新增（本层测试重点）

- [ ] `parse_complete_post_notify` — 完整请求解析出 method/path/headers/body
- [ ] `parse_tcp_split_header_body` — ★ header 与 body 分两次到达（`find_subslice` 只找到部分时继续读）
      （回归 `notify_server.rs:79-107` 的循环补齐全逻辑）
- [ ] `parse_tcp_split_three_ways` — 分三段到达
- [ ] `parse_missing_content_length` — 无 `Content-Length` 头 → 视为 0
- [ ] `parse_content_length_larger_than_body` — 声明 100 实际 10 → 需继续读（不应越界 panic）
- [ ] `parse_content_length_smaller_than_body` — 声明 10 实际 100 → 截断到 10
- [ ] `parse_header_case_insensitive` — `content-length` / `Content-Length` / `CONTENT-LENGTH` 均可
- [ ] `parse_rejects_non_post` — GET / PUT → 404
- [ ] `parse_rejects_wrong_path` — `POST /foo` → 404
- [ ] `parse_rejects_invalid_json` — body 非 JSON → 400
- [ ] `parse_rejects_empty_text` — `{"text":""}` → 400
- [ ] `parse_rejects_text_over_limit` — ★ 121 个中文字符 → 400
      （断言按 `chars().count()` 而非 `len()`，回归中文计数，对应 B4）
- [ ] `parse_accepts_text_at_limit` — 120 字 → 200（边界）
- [ ] `parse_rejects_wrong_host` — ★ `Host: evil.com` → 403（新增安全，P0-3）
- [ ] `parse_rejects_body_over_8kb` — ★ `Content-Length: 9000` → 413（新增安全，P0-3）
- [ ] `parse_extracts_action_and_duration` — `action` / `duration` 正确解析
- [ ] `parse_peer_closed_mid_request` — 对端在 header 中途关闭 → 不 hang、返回错误
- [ ] `render_response_status_codes` — 200/400/403/404/413 的响应字节正确（含 `Content-Length`）

### 2.7 `domain/gallery/index.rs` ★ 新增

- [ ] `name_fallback_zh` — `localized_names.zh` 存在 → 用中文
- [ ] `name_fallback_en` — `zh` 为空、`en` 存在 → 用英文
- [ ] `name_fallback_name` — 两者皆空 → 用 `name`
- [ ] `name_fallback_slug` — 全空 → 用 `slug`
- [ ] `skips_empty_slug` — `slug` 为空的条目被过滤
- [ ] `preview_url_format` — URL 拼接格式正确（含 slug 转义）
- [ ] `source_urls_format` — `pet.json` / `spritesheet.webp` URL 格式

**L1 小计：约 80 个用例**

---

## 3. L2 Rust 集成测试（`tests/`）

> 位置：`src-tauri/tests/*.rs`
> 前置：REFACTOR_PLAN Phase 2.1（`lib.rs`）+ Phase 4

### 3.1 `tests/http_server.rs` ★ 真端口、真 socket

- [ ] `server_binds_and_responds_ok` — 起服务 → 真实 TCP 发送 `POST /notify` → 断言 `200` + body `ok`
- [ ] `server_rejects_oversized_body` — 发送声明 100MB 的请求（不真发满）→ 断言 413
- [ ] `server_rejects_wrong_host` — `Host: evil.com` → 403
- [ ] `server_rejects_slowloris` — 分 100 次、每次 1 字节发送 → 断言 10s 内被断开（总超时）
      （回归 P0-3 第 4 点：`set_read_timeout` 只约束单次 read）
- [ ] `server_handles_concurrent_connections` — 32 个并发连接全部成功；第 33 个被限流
- [ ] `server_emits_event_to_app` — ⚠️ 需要 mock AppHandle，或改为断言 `emit` 被调用（用 channel 注入）
- [ ] `server_reports_bind_failure` — 先占用 8756 端口 → 启动服务 → 断言错误被上报而非静默
      （回归 P0-3 第 5 点：当前只 `eprintln!`）

### 3.2 `tests/pet_storage.rs` ★ 真 tempdir、真文件系统

- [ ] `import_then_list_then_delete` — 导入 zip → `list_imported_pets` 返回 1 条 → 删除 → 目录消失 → list 返回 0
- [ ] `import_all_bundled_zips` — ★ 遍历 `pet/*.zip`（9 个真实包）逐个导入并断言成功
      （回归风险 RK7：解压行为变更会导致已有包导入失败）
- [ ] `import_rejects_zip_without_pet_json`
- [ ] `import_rejects_zip_without_webp`
- [ ] `import_rejects_invalid_pet_id` — zip 内 `pet.json` 的 id 含 `../` → 拒绝
- [ ] `import_overwrites_existing_same_id` — 同 id 二次导入 → 旧目录被清理、新文件就位
- [ ] `update_metadata_persists` — 改名字/描述 → 重读 `pet.json` 断言已写回
- [ ] `update_metadata_rejects_missing_pet` — 不存在的 id → `Err`（不是静默成功）
- [ ] `delete_nonexistent_is_idempotent` — 删除不存在的宠物 → `Ok`
- [ ] `download_online_pet_id_matches_dir` — ★ **回归 P0-2**：mock HTTP 返回 `id != slug` 的 `pet.json`
      → 下载 → 断言 `PetDefJson.id == 目录名` → 删除 → 断言目录确实消失

### 3.3 `tests/zip_slip.rs` ★ 安全回归

- [ ] `rejects_parent_traversal` — zip 含 `../../../evil.txt` → 断言未在 `pets_root` 外产生文件
- [ ] `rejects_absolute_path` — zip 含 `/tmp/evil.txt` → 断言未逃逸
- [ ] `rejects_symlink_entry` — zip 含指向 `/etc/passwd` 的符号链接 → 断言未被创建为软链
- [ ] `rejects_zip_bomb` — 高度可压缩的大文件（如 1GB 的 0x00）→ 断言在解压上限处中止
- [ ] `rejects_too_many_entries` — 10000 个空文件条目 → 断言在条目上限处中止

### 3.4 `tests/contract_gallery.rs`（可并入 L5）

- [ ] `gallery_index_matches_fixture` — 用 `tests/fixtures/pets.json` 真实索引片段 → 断言映射结果快照

**L2 小计：约 25 个用例**

---

## 4. L3 前端单元测试（`model/` + `shared/`）

> 位置：`src/**/*.spec.ts`（与源码同目录）或 `src/**/__tests__/*.spec.ts`
> 工具：`vitest` + `jsdom`；纯函数不需要 DOM
> 前置：REFACTOR_PLAN Phase 6

### 4.1 `features/pet/model/frame.spec.ts`

- [ ] `seqFor_returns_idle_for_idle_state`
- [ ] `seqFor_returns_talk_for_talk_state`
- [ ] `seqFor_returns_action_when_exists`
- [ ] `seqFor_falls_back_to_idle_for_unknown_action`
- [ ] `seqFor_returns_null_when_no_pet`
- [ ] `frameBounds_computes_rows_cols_from_natural_size` — 1920×2288 / 192×208 → 10 列 × 11 行
- [ ] `frameBounds_handles_non_standard_frame` — ★ 1536×1536 / 256×256 → 6 列 × 6 行（P0-1）
- [ ] `frameBounds_returns_zero_for_tiny_image` — 图小于一帧 → cols=0（不应崩溃）
- [ ] `isFrameInBounds_false_for_row_overflow` — ★ 9 行图访问 row 10 → false（回归 E2）
- [ ] `isFrameInBounds_false_for_col_overflow` — count=8 但 cols=6 时 col=7 → false

### 4.2 `features/pet/model/actionScheduler.spec.ts` ★ 核心状态机

- [ ] `play_sets_current_action`
- [ ] `play_ignores_unknown_action_and_goes_idle`
- [ ] `finish_returns_to_idle`
- [ ] `randomTick_filters_pool_by_available_actions` — 宠物无 `jump` 时不会随机到 `jump`
- [ ] `randomTick_skips_when_talking` — ★ `talk` 状态下不触发随机动作（回归 A9）
- [ ] `randomTick_is_deterministic_with_injected_rng` — 注入固定 rng → 结果可预测
- [ ] `randomTick_empty_pool_keeps_idle` — 宠物无任何随机动作 → 保持 idle
- [ ] `hover_plays_waiting` — 进入 `waiting`
- [ ] `hover_ignored_while_talking` — `talk` 时 hover 不打断气泡
- [ ] `unhover_returns_idle_only_if_waiting` — 若已切到别的动作，不误清
- [ ] **`timers_do_not_cross_cancel`** — ★ 回归 `PetHost.vue:30-31` 注释记录的
      「动作定时器与随机定时器共用导致互相 clearTimeout」的历史 bug：
      播放 A 动作期间调度随机动作，断言 A 的完成回调未被取消
- [ ] `duration_defaults_from_seq` — 未指定 `durationMs` 时 = `count / fps * 1000`

### 4.3 `features/pet/model/geometry.spec.ts` ★ 平台时序回归

- [ ] `computeHitRects_includes_pet_and_bubble`
- [ ] `computeHitRects_pads_by_scale` — ★ `scale=1.3` 时 padding = `28 * 1.3`（回归 `PetHost.vue:214-215` 硬编码魔数）
- [ ] `computeHitRects_empty_when_pet_missing_but_bubble_present` — ★ 回归启动竞态：
      宠物未加载完时上报 `[]` 保持整窗可交互（对应 `PetHost.vue:259-267` 注释记录的"偶尔异常"）
- [ ] `computeHitRects_empty_when_both_missing`
- [ ] `computeHitRects_uses_cached_leaving_bubble_rect` — ★ 气泡离场中（ref 为 null）时用缓存矩形
      （回归 `PetHost.vue:165-195` 记录的"气泡淡出到一半被硬裁"）
- [ ] `padRect_expands_all_four_sides` — 四向外扩而非只扩下方
- [ ] `padRect_handles_zero_size`

### 4.4 `features/notify/model/notifyQueue.spec.ts` ★

- [ ] `enqueue_adds_to_queue`
- [ ] `enqueue_ignores_empty_text`
- [ ] `enqueue_assigns_increasing_ids`
- [ ] `next_pops_fifo` — 多条按入队顺序播放
- [ ] `next_returns_null_when_empty`
- [ ] `action_takes_priority_over_bubble` — ★ 有 `action` 时先播动作再显示气泡
      （回归 `PetHost.vue:129-137` 注释记录的"选了动作没生效"）
- [ ] **`duration_is_preserved`** — ★ `{duration: 1000}` → `durationOf` 返回 1000（**回归 P0-4**）
- [ ] `duration_defaults_to_4000` — 未指定时返回 `DEFAULT_BUBBLE_MS`
- [ ] `queue_does_not_drop_when_current_active` — 播放中入队不丢消息

### 4.5 `features/gallery/model/filter.spec.ts`

- [ ] `filter_matches_name_case_insensitive`
- [ ] `filter_matches_author`
- [ ] `filter_matches_category`
- [ ] `filter_empty_keyword_returns_all`
- [ ] `filter_trims_keyword`
- [ ] `filter_no_match_returns_empty`

### 4.6 `features/pet/model/dialogues.spec.ts`

- [ ] `pickDialogue_builtin_pet_uses_own_lines` — miku / ryujinmaru / Seedy 各自一套
- [ ] `pickDialogue_external_pet_uses_generic`
- [ ] `pickDialogue_unknown_action_returns_empty`
- [ ] `pickDialogue_deterministic_with_injected_rng`

### 4.7 `shared/utils/base64.spec.ts`

- [ ] `arrayBufferToBase64_small` — < 0x8000 单块
- [ ] `arrayBufferToBase64_large_chunked` — ★ > 0x8000 时分块正确（回归 `PetSettings.vue:311` 的 `String.fromCharCode(...)` 栈溢出风险）
- [ ] `arrayBufferToBase64_empty`
- [ ] `arrayBufferToBase64_matches_known_vector`

### 4.8 `shared/ipc/events.spec.ts` ★ 防回环

- [ ] `emit_attaches_source_window_id`
- [ ] `onCrossWindow_skips_own_source` — ★ 自己发的事件不会被自己接收（**替代现有"值相等防回环"**）
- [ ] `onCrossWindow_receives_other_source`
- [ ] `disposeAll_unsubscribes_everything` — ★ 回归 P1-1（8 个监听未清理）
- [ ] `onCrossWindow_handles_payload_variants` — string / boolean / object

### 4.9 `shared/config/constants.spec.ts`

- [ ] `constants_match_rust_bindings` — ★ 与 `src/bindings/constants.ts`（Rust 生成）逐字段断言相等
      （若 L5 契约测试已覆盖可省略此处，二选一）

**L3 小计：约 60 个用例**

---

## 5. L4 前端组件测试（@vue/test-utils）

> 前置：REFACTOR_PLAN Phase 7 + `src/test/mocks/tauri.ts`
> 环境：`jsdom`（canvas 需 mock `getContext`）

### 5.1 `SpritePet.spec.ts`

- [ ] `renders_canvas_with_frame_size_x_scale` — `width/height` 属性正确
- [ ] `loads_image_from_public_path_for_builtin_pet` — 内置宠物 → `/pets/xxx/spritesheet.webp`
- [ ] `loads_image_from_data_url_for_external_pet` — 外部宠物 → base64 data URL
- [ ] `advances_frame_on_tick` — 用 `vi.useFakeTimers` + 手动推进 rAF
- [ ] **`resets_frame_index_on_state_change`** — ★ 切换动作时 `frameIdx` 归零
      （回归 `SpritePet.vue:133-139` 注释记录的「新动作 row + 旧 frameIdx → 越界 → 宠物闪没」）
- [ ] **`redraws_immediately_after_scale_change`** — ★ 回归 P1-3（缩放闪空白帧）
- [ ] **`clears_canvas_when_pet_changes`** — ★ 回归 P1-4（切宠物残留上一只）
- [ ] `keeps_previous_frame_when_frame_out_of_bounds` — 越界不清空画布
- [ ] `cancels_raf_on_unmount`

### 5.2 `PetBubble.spec.ts`

- [ ] `renders_text`
- [ ] **`disappears_after_custom_duration`** — ★ 回归 P0-4：`duration=1000` → 1s 后消失
- [ ] `disappears_after_default_duration` — 4s
- [ ] `shows_next_queued_item_after_current`
- [ ] `emits_leave_with_cached_rect` — ★ 离场时提供缓存矩形（供 `useHitRects` 用）
- [ ] `applies_translate_x_offset_for_chat_bubble` — `--bubble-x` 生效
- [ ] `has_no_max_width_conflict_at_high_scale` — ⚠️ scale=1.3 时不与容器 280px 冲突（若已修）

### 5.3 `PetListPanel.spec.ts`

- [ ] `renders_all_pets`
- [ ] `highlights_current_pet`
- [ ] `shows_edit_delete_only_for_external_pets`
- [ ] `emits_select_on_click`
- [ ] **`requires_second_click_to_confirm_delete`** — 回归 C7
- [ ] **`resets_confirm_state_after_3s`** — 回归 C7 的超时复位

### 5.4 `ScaleSlider.spec.ts`

- [ ] `maps_scale_to_percent_position`
- [ ] **`snaps_to_0_05_steps`** — 回归 C4
- [ ] `clamps_below_min_and_above_max`
- [ ] `emits_change_during_pointer_move`
- [ ] `releases_pointer_capture_on_up`

### 5.5 `OnlineGalleryDialog.spec.ts`

- [ ] `shows_loading_state`
- [ ] `shows_error_state_when_load_fails`
- [ ] `shows_empty_state_when_no_match`
- [ ] `shows_reinstall_label_for_installed_slug` — ⚠️ 依赖 P0-2 修复（id 与 slug 一致）
- [ ] `shows_downloading_state_per_card`
- [ ] `filters_by_keyword`

### 5.6 `SettingsWindow.spec.ts`

- [ ] `loads_manifest_on_mount_when_store_empty`
- [ ] `displays_version_from_tauri`
- [ ] **`unsubscribes_all_events_on_unmount`** — ★ 回归 P1-1
- [ ] `does_not_start_window_drag_on_interactive_elements` — 回归 C10

### 5.7 `useHitRects.spec.ts`（composable）

- [ ] `reports_rects_after_next_tick` — ★ 回归 `reportInteractiveRectsSettled` 的 `await nextTick()`
- [ ] `reports_again_after_settle_timeout` — 200ms 兜底
- [ ] `does_not_report_when_not_tauri`
- [ ] `sends_empty_when_pet_not_ready` — 回归启动竞态

### 5.8 `useTauriEvent.spec.ts`（composable）

- [ ] `registers_on_mount`
- [ ] **`unlistens_on_unmount`** — ★ 回归 P1-1
- [ ] `passes_payload_to_handler`

**L4 小计：约 40 个用例**

---

## 6. L5 契约测试（Rust ↔ TS）

> **目的**：杜绝"Rust 改了字段名，前端静默拿到 `undefined`"这类跨语言 bug。
> 本项目已有苗头：`PetDefJson` 用 `display_name`（snake_case），前端 `PetDef` 用 `displayName`，
> 目前靠**人工记忆**保持一致（`store/pet.ts:151-161` 手写了逐字段映射，极易漏改）。

### 6.1 方案

1. Rust 侧引入 `ts-rs`（dev-dependency）：
   ```rust
   #[derive(Serialize, ts_rs::TS)]
   #[ts(export, export_to = "../../src/bindings/")]
   pub struct PetDefJson { /* ... */ }
   ```
2. 生成物：`src/bindings/PetDefJson.ts`、`NotifyPayload.ts`、`ErrorCode.ts`、`Platform.ts`、`Constants.ts`
3. 前端**改为从 bindings import**，删除手写的重复接口定义
4. CI job：
   ```yaml
   - run: cargo test export_ts_bindings
   - run: git diff --exit-code src/bindings/ # 有漂移即失败
   ```

### 6.2 覆盖的 DTO

- [ ] `PetDefJson` / `FrameSeqJson` / `Frame`（★ 含 P0-1 新增的 `frame`）
- [ ] `NotifyPayload`（★ 含 `duration`）
- [ ] `ErrorCode`（枚举，前端 `messages.ts` 必须穷举匹配）
- [ ] `OnlinePetMeta`
- [ ] `Platform`
- [ ] `AppState`（Phase 8 引入）
- [ ] **常量**：`FRAME` / `SCALE` / `WINDOW` / `NOTIFY` / `BUBBLE`（由 Rust `domain` 常量生成，消灭两边各写一份）

### 6.3 断言

- [ ] `bindings_are_up_to_date` — `git diff --exit-code`
- [ ] `error_code_enum_is_exhaustive_in_ts` — TS 侧用 `Record<ErrorCode, string>` + `satisfies` 保证穷举
      （新增错误码时编译期报错）

---

## 7. L6 E2E（tauri-driver）

> 工具：`tauri-driver` + WebdriverIO（Tauri 官方方案）
> 前置：REFACTOR_PLAN Phase 9.6；仅 macOS runner（Windows runner 无窗口会话，暂不支持）
> **不设为 required**，失败重试 2 次

### 7.1 冒烟用例（对应功能基线）

- [ ] **E2E-1 启动**（A1/A2）：启动 App → 断言 main 窗口存在、可见、位置在右下象限、无边框
- [ ] **E2E-2 切换宠物**（C3）：打开设置 → 点击「龙神丸」→ 断言设置窗高亮变化
      → 断言 main 窗口 canvas 尺寸变化 → **关闭设置 → 重启 App → 断言仍是「龙神丸」**（持久化）
- [ ] **E2E-3 缩放**（C4/A12）：拖动滑块到 130% → 断言 main 窗口尺寸变化
      → **断言窗口右下角坐标变化 < 2px**（锚点不漂移，回归 RK4）
- [ ] **E2E-4 通知端到端**（B1/B3）：
      `curl -X POST http://127.0.0.1:8756/notify -d '{"text":"e2e-test"}'`
      → 断言气泡 DOM 出现且文本正确 → 断言 4s 后消失
- [ ] **E2E-5 导入 zip**（C6）：通过文件输入注入 `pet/miku.codex-pet.zip`
      → 断言列表新增 → 断言自动切换 → 点击删除两次 → 断言列表移除
- [ ] **E2E-6 显隐**（C5/A1）：切换「显示宠物」→ 断言 main 窗口隐藏 → 再切换 → 断言显示

### 7.2 工程约束

- [ ] 每个用例独立启动/销毁 App 实例（避免状态污染）
- [ ] 端口 8756 冲突时自动跳过 E2E-4（CI 环境不保证端口空闲）
- [ ] 超时：单用例 ≤ 60s，整体 ≤ 5min
- [ ] 失败时自动截图 + 导出 webview console 日志作为 artifact

---

## 8. 手动测试清单（自动化不可覆盖）

> 落地为 `docs/refactor/MANUAL_TEST.md`，**每次发版前逐项勾选**。
> 这些是"手感"与"平台视觉"类验证，自动化断言不了，但一旦回归用户立刻能感觉到。

### 8.1 macOS（需覆盖 12 / 13 / 14 三个大版本）

- [ ] 穿透手感：鼠标在宠物上可交互，移开后点击能穿透到下层窗口（Finder/浏览器）
- [ ] 透明区无残留：气泡/宠物阴影未被 `setIgnoresMouseEvents` 影响
- [ ] 拖拽跟手度：原生 `setFrameOrigin` 拖拽无延迟、无抖动
- [ ] 不抢焦点：点击宠物时当前活跃 App 不失去焦点
- [ ] 全 Space 常驻：切换虚拟桌面、进入全屏 App，宠物表现一致且不闪烁
      （回归 `macos_pet.rs:388-400` 记录的"个别桌面边框闪烁"）
- [ ] Dock 行为：开机为纯托盘（无 Dock 图标）；打开设置→出现在 Dock；关闭设置→退出 Dock
- [ ] 开机自启：系统设置 → 通用 → 登录项 中可见 PetBuddy
- [ ] Retina / 外接非 Retina 显示器切换：宠物与气泡位置不错位

### 8.2 Windows（需覆盖 10 / 11；DPI 100% / 125% / 150%）

- [ ] 区域裁剪：气泡出现/消失时无"隐约的 Windows 窗口"轮廓
      （回归 `main.rs:111-121` 与 `windows_pet.rs:83-89` 记录的两个 bug）
- [ ] 阴影未被裁：气泡四周阴影完整，无缺角/断层
- [ ] 气泡淡出同步：本体与箭头同时消失，无"箭头晚消失 ~120ms"
      （回归 `PetHost.vue:290-294` 注释记录的 340ms→200ms 兜底调整）
- [ ] 跨屏 DPI：把宠物拖到与主屏缩放不同的副屏，命中区域与视觉一致
      （回归 `windows_pet.rs:17-22` 记录的 `GetDeviceCaps` vs `GetDpiForWindow` 陷阱）
- [ ] 隐藏时无边框闪现：切换「显示宠物」→ 关闭，无窗口边框一闪而过
- [ ] 设置窗圆角：DWM 圆角与 CSS 圆角视觉一致，四角无黑边
- [ ] 开机自启：注册表 `HKCU\...\Run` 存在 `PetBuddy` 且带 `--autostart`；重启后自动启动
- [ ] 任务栏：宠物不出现在任务栏；设置窗口正常出现在任务栏

### 8.3 通用

- [ ] 多个外部宠物（≥5 个）连续快速切换，无残影、无错帧、无内存持续增长
- [ ] 连续发 20 条通知，队列不丢、顺序正确、气泡不叠
- [ ] 长时间常驻（≥8 小时）无内存泄漏、CPU 占用正常（macOS 应可休眠）
- [ ] 导入 9 个 `pet/*.zip` 全部成功且动画正确
- [ ] 老版本升级：从 v0.1.6x 升级后，宠物选择/缩放/显隐/窗口位置全部保持

---

## 9. 覆盖率门禁

### 9.1 目标

| 范围                            | 目标 line 覆盖率 | 说明                                                             |
| ------------------------------- | ---------------- | ---------------------------------------------------------------- |
| `src-tauri/src/domain/**`       | **≥ 90%**        | 纯逻辑，必须高覆盖                                               |
| `src-tauri/src/infra/**`        | ≥ 70%            | 有 IO，靠集成测试补                                              |
| `src-tauri/src/platform/**`     | 豁免             | FFI + 系统 API，无法自动化，标注 `#[cfg_attr(coverage, ignore)]` |
| `src-tauri/src/commands/**`     | ≥ 60%            | 薄层，靠集成测试间接覆盖                                         |
| `src/shared/**`                 | **≥ 90%**        | 纯逻辑                                                           |
| `src/features/**/model/**`      | **≥ 90%**        | 纯逻辑                                                           |
| `src/features/**/store/**`      | ≥ 80%            |                                                                  |
| `src/features/**/components/**` | ≥ 60%            |                                                                  |
| `src/windows/**`                | ≥ 40%            | 编排层，靠 E2E 覆盖                                              |
| **整体（各语言）**              | **≥ 60%**        |                                                                  |

### 9.2 配置

`vitest.config.ts`：

```ts
coverage: {
  provider: 'v8',
  thresholds: {
    global: { lines: 60, functions: 60, branches: 55 },
    // 逐目录更高要求
    'src/shared/': { lines: 90 },
    'src/features/**/model/': { lines: 90 },
  },
}
```

CI：

```yaml
- run: npm run test:cov
- run: cargo llvm-cov --lcov --output-path lcov.info # 或 cargo-tarpaulin
- uses: codecov/codecov-action@v4
-  # 覆盖率相对 base 下降 > 2% → fail
```

### 9.3 原则

- **覆盖率是信号不是目标**。禁止为凑数写 `expect(true).toBe(true)` 式的空断言。
- 每条测试必须**能失败**：写完后故意改坏被测函数，确认测试变红，再改回来。
- 平台相关代码（`platform/**`、`usePetDrag` 的原生分支）**豁免覆盖率**，但必须有对应手动测试项。

---

## 10. CI 编排

```yaml
# .github/workflows/quality.yml   （required）
on: [push, pull_request]
jobs:
  frontend:
    - checkout / setup-node 20 / npm ci
    - npm run typecheck
    - npm run lint
    - npm run format:check
    - npm run test:cov          → 上传覆盖率
  rust:
    - setup-rust stable / cargo cache
    - cargo fmt --check
    - cargo clippy --all-targets -- -D warnings
    - cargo test --all-features
```

```yaml
# .github/workflows/contract.yml  （required）
on: [push, pull_request]
jobs:
  contract:
    - cargo test export_ts_bindings
    - git diff --exit-code src/bindings/ # 漂移即失败
```

```yaml
# .github/workflows/e2e.yml       （非 required，重试 2 次）
on: [push, pull_request]
jobs:
  e2e:
    runs-on: macos-14
    - 安装 tauri-driver（brew install tauri-driver）或下载二进制
    - npm run build
    - wdio run wdio.conf.ts
    - 失败时上传截图 + console 日志
```

```yaml
# .github/workflows/build.yml     （三平台矩阵，PR 只构建不发布）
on: [push, pull_request]
jobs:
  build:
    strategy:
      matrix:
        os: [macos-14, windows-latest, ubuntu-22.04]
```

```yaml
# .github/workflows/build-windows.yml  （保留现有，仅复用 bump-version.mjs）
# .github/workflows/pages.yml          （保留，不变）
```

**分支保护规则**：

- Required：`quality` / `contract`
- 可选：`e2e` / `build`
- 禁止 force push 到 `main`
- 合并前必须至少 1 个 review（个人项目可设为 self-merge + CI 全绿）

---

## 11. 测试数据与 Fixture 清单

### 11.1 需新增的 fixture

| 路径                                                  | 用途         | 说明                                  |
| ----------------------------------------------------- | ------------ | ------------------------------------- |
| `src-tauri/tests/fixtures/webp/vp8_standard.webp`     | L1 单测      | 最小 VP8 lossy，如 16×16              |
| `src-tauri/tests/fixtures/webp/vp8l_standard.webp`    | L1 单测      | 最小 VP8L                             |
| `src-tauri/tests/fixtures/webp/vp8x_standard.webp`    | L1 单测      | 最小 VP8X                             |
| `src-tauri/tests/fixtures/webp/vp8_wide_16384.webp`   | ★ L1 回归    | 宽度 ≥16384（历史 bug）               |
| `src-tauri/tests/fixtures/webp/not_webp.png`          | L1 单测      | 非 webp 输入                          |
| `src-tauri/tests/fixtures/pets/standard_11row.zip`    | L1/L2        | 标准 11 行包                          |
| `src-tauri/tests/fixtures/pets/short_9row.zip`        | ★ L1/L2 回归 | 9 行包（miku 场景）                   |
| `src-tauri/tests/fixtures/pets/nonstandard_frame.zip` | ★ L1         | 256×256 / 6 列（P0-1）                |
| `src-tauri/tests/fixtures/pets/no_pet_json.zip`       | L2 负例      |                                       |
| `src-tauri/tests/fixtures/pets/no_webp.zip`           | L2 负例      |                                       |
| `src-tauri/tests/fixtures/pets/bad_id.zip`            | L2 负例      | id 含 `../`                           |
| `src-tauri/tests/fixtures/zip_slip_parent.zip`        | ★ L2 安全    | 含 `../../evil.txt`                   |
| `src-tauri/tests/fixtures/zip_slip_absolute.zip`      | ★ L2 安全    | 含 `/tmp/evil.txt`                    |
| `src-tauri/tests/fixtures/zip_bomb.zip`               | L2 安全      | 高压缩比大文件                        |
| `src-tauri/tests/fixtures/gallery/pets.json`          | L1/L2        | 真实索引片段（含 zh/en/空 三种）      |
| `pet/*.zip`（现有 9 个）                              | ★ L2 回归    | **直接复用为测试输入，零成本**（RK7） |

### 11.2 生成方式

- webp fixture：用 `cwebp` 或 Python `PIL` 生成，提交时控制在 **每个 < 5KB**
- zip fixture：用 `zip` 命令行生成后提交；`zip_slip` 类需手工构造（或用 Python `zipfile` 写路径）
- gallery fixture：从 awesome-codex-pet 真实索引截取 10 条（脱敏后提交）

### 11.3 Mock 清单（前端）

| Mock                | 位置                      | 覆盖对象                                                |
| ------------------- | ------------------------- | ------------------------------------------------------- |
| `mockTauriInvoke`   | `src/test/mocks/tauri.ts` | `@tauri-apps/api/core`                                  |
| `mockTauriEvent`    | 同上                      | `@tauri-apps/api/event`（listen/emit + 手动触发）       |
| `mockTauriWindow`   | 同上                      | `@tauri-apps/api/window`（startDragging / onMoved）     |
| `mockCanvasContext` | `src/test/setup.ts`       | `HTMLCanvasElement.getContext('2d')`（jsdom 无 canvas） |
| `mockImageOnload`   | `src/test/setup.ts`       | `Image` 的 load/error，用于 `SpritePet` 测试            |
| `mockClipboard`     | `src/test/setup.ts`       | `navigator.clipboard`（curl 复制测试）                  |

---

## 附：测试落地顺序（与重构 Phase 对齐）

| 重构阶段 | 同步落地的测试                                        |
| -------- | ----------------------------------------------------- |
| Phase 0  | 测试框架就位，0 用例（仅跑通空套件）                  |
| Phase 1  | 每个 bug 修复带 1~3 条回归测试（约 15 条）            |
| Phase 2  | **L1 主体**：`domain/` 全量单测（约 80 条）★ 收益峰值 |
| Phase 3  | L1 补充（platform trait 的 mock 实现）                |
| Phase 4  | **L2 主体**：集成测试（约 25 条）                     |
| Phase 5  | mock 层就位                                           |
| Phase 6  | **L3 主体**：前端纯逻辑单测（约 60 条）★ 收益峰值     |
| Phase 7  | **L4 主体**：组件测试（约 40 条）                     |
| Phase 8  | L3/L4 补充（状态同步、迁移）                          |
| Phase 9  | **L5 契约 + L6 E2E + 覆盖率门禁**                     |
| Phase 10 | CI 编排 + 手动清单文档化                              |

**合计约 230 条自动化用例**，其中约 60 条是针对本项目**已实际发生过或高概率复发**的 bug 的回归测试。
