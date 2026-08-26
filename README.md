# PetBuddy 🐾

跨平台桌面宠物 + 通用通知接收器（基于 Tauri 2 + Vue 3）。

一只卡通宠物常驻桌面右下角（透明无边框窗口），空闲时随机做小动作，支持拖动、点击交互、缩放、切换，并可作为系统的「通知播报员」——任意外部应用通过本地 HTTP 接口即可让它说话 + 做动作。

## 功能

- **宠物常驻**：一只卡通宠物常驻桌面右下角，透明无边框、`alwaysOnTop` 窗口（自动避开 Dock / 任务栏）
- **随机小动作**：空闲时随机播放挥手 / 蹦跳 / 张望 / 工作 / 转头 / 失败 / 等待等动作
- **交互**：
  - 拖动 → 跑步动画（左/右方向自动判断）
  - 单击 → 随机动作 + 性格搭话气泡
  - 双击 → 打开设置窗口
  - 悬停 → 张望（宠物 / 气泡可交互，透明区域点击穿透到桌面）
- **缩放**：80% ~ 130% 可调（每 0.05 档），窗口跟随缩放（不重置位置）
- **显示 / 隐藏**：托盘菜单「显示 / 隐藏宠物」或设置窗口切换，状态持久化
- **宠物切换**：内置 3 只（Miku / 龙神丸 / Seedy）+ 本地 zip 导入 / 删除 / 编辑 + 在线画廊下载
- **专属台词**：每只内置宠物有独立性格的搭话气泡（`src/pets/dialogues.ts`）
- **像素穿透**：
  - macOS：NSTimer 每 50ms 轮询鼠标位置，命中宠物 / 气泡矩形则交互，否则 `setIgnoresMouseEvents` 穿透
  - Windows：`SetWindowRgn` 把窗口裁成「宠物 + 气泡」圆角矩形，区域外点击穿透（支持 Per-Monitor DPI）
- **开机自启**：托盘菜单「开机自启」开关（`CheckMenuItem`，勾选态反映当前状态，切换后自动重建菜单刷新勾选）
  - macOS：Apple 官方 `SMAppService` 登录项（需打包后的 `.app` 生效）
  - Windows：在用户「启动」文件夹写入 / 删除指向当前 exe 的 `.lnk` 快捷方式（零依赖，手写 Shell Link 二进制，无需 COM / 注册表）
- **通用通知接口**：本地 HTTP 服务（`127.0.0.1:8756`），任意外部应用可发通知（见下文）
- **外部宠物自适应**：导入时解析精灵图实际尺寸，自动修正越界帧，避免异常布局导致宠物消失
- **在线画廊**：内置接入 [awesome-codex-pet](https://github.com/legeling/awesome-codex-pet) 索引，可直接浏览 / 下载社区宠物（预览图来自 codexpet.top）

## 开发运行

```bash
npm install
bash ./scripts/tauri-dev.sh    # Tauri 桌面版（前端 dev + 原生窗口）
```

其它脚本（均在 `scripts/` 目录）：

```bash
bash ./scripts/dev.sh          # 仅前端 dev（浏览器，无 Tauri 窗口/托盘/穿透）
bash ./scripts/build.sh        # 生产构建（自动 patch 自增版本号 + 前端 build + Tauri 打包）
bash ./scripts/install.sh      # 安装依赖
```

> 脚本会自动加载 Homebrew 的 node@20 LTS，无需手动切换 Node 版本。

## 打包

```bash
bash ./scripts/build.sh            # 自动按 patch 自增版本号（0.1.14 → 0.1.15）
bash ./scripts/build.sh 0.2.0      # 或手动指定版本号
```

版本号会同步写回 `package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` 三处（不会自动 git commit）。产物生成在 `src-tauri/target/release/bundle/`（`.app` / `.dmg` 等，由 `tauri.conf.json` 的 `bundle.targets: "all"` 决定）。

## 双窗口架构

- **main**（宠物窗口）：无边框、透明、置顶、320×380，右下角常驻，承载 `PetHost` + `SpritePet`
- **settings**（设置窗口）：680×500，居中、默认隐藏；双击宠物或托盘「打开设置」时打开；关闭时隐藏而非销毁，并记住最后拖动位置，下次打开恢复

两个窗口是独立 webview，各自持有独立 store 实例。跨窗口状态（切换宠物 / 缩放 / 显隐）通过 Tauri 事件（`pet-switch` / `pet-scale` / `pet-visible`）广播同步，规避前端 emit 跨窗口不生效的问题。macOS 打开设置时切换 Dock 图标为 `Regular`（显示），关闭时切回 `Accessory`（隐藏）。

## 通知接口（供外部应用调用）

```
POST http://127.0.0.1:8756/notify
Content-Type: application/json

{
  "text": "通知正文",      // 必填，最多 120 字（中文按 1 字符计，超限返回 400）
  "action": "wave",        // 可选：宠物动作（wave/jump/failed/waiting/working/look）
  "duration": 4000         // 可选：气泡显示时长(ms)，默认 4000
}
```

示例（curl）：

```bash
# 只发文字
curl -X POST http://127.0.0.1:8756/notify \
  -H "Content-Type: application/json" \
  -d '{"text":"你好，这是一条测试通知！"}'

# 带宠物动作 + 自定义时长
curl -X POST http://127.0.0.1:8756/notify \
  -H "Content-Type: application/json" \
  -d '{"text":"太棒了！","action":"jump","duration":6000}'
```

Python：

```python
import requests
requests.post("http://127.0.0.1:8756/notify",
              json={"text": "来自 Python 的通知", "action": "wave"})
```

Node.js：

```js
await fetch("http://127.0.0.1:8756/notify", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ text: "来自 Node 的通知", action: "jump" }),
})
```

> 通知服务用标准库 `TcpListener` 手写极简 HTTP（仅解析 `POST /notify` 的 JSON body），零额外网络依赖。前端设置窗口的「测试通知」则走 Tauri IPC（`push_notify` command），与 HTTP 广播共用 `notify-push` 事件名，宠物气泡无需区分来源。

## 项目结构

```
src/                       # Vue 3 前端
  App.vue                  # 入口，按窗口 label 渲染 PetHost / PetSettings
  main.ts / style.css      # 引导 + 全局样式
  tauri.ts                 # Tauri 封装（isTauri / emitEvent / openSettingsWindow）
  components/
    PetHost.vue            # 宠物宿主（main 窗口）：气泡 + 精灵播放 + 拖拽 + 穿透上报
    SpritePet.vue          # 精灵图播放器（按 manifest 帧序列逐帧）
    PetSettings.vue        # 设置窗口：宠物切换 / 缩放 / 显隐 / 自启 / 导入 / 在线画廊
  store/
    pet.ts                 # 宠物状态（双窗口共享逻辑 + 跨窗口广播）
    notify.ts              # 本地通知（pending 队列 + IPC 广播）
  pets/
    manifest.json          # 内置宠物清单（帧布局：192×208、8 列）
    dialogues.ts           # 各宠物性格搭话台词
src-tauri/                 # Rust 后端（Tauri 2）
  src/
    main.rs                # 入口：窗口 + 托盘 + 事件 + 命令注册
    macos_pet.rs           # macOS 像素穿透（NSTimer 50ms 轮询 setIgnoresMouseEvents）
    windows_pet.rs         # Windows 穿透（SetWindowRgn 圆角矩形裁切 + Per-Monitor DPI）
    pet_import.rs          # 外部宠物导入（zip 解压 + webp 尺寸解析 + 越界修正 + 在线画廊）
    autostart.rs           # 开机自启（macOS SMAppService 登录项 / Windows Startup 文件夹 .lnk）
    notify_server.rs       # 本地通知 HTTP 服务（127.0.0.1:8756）
  icons/                   # 应用图标 + 托盘图标
public/pets/               # 内置宠物精灵图资源（miku / ryujinmaru / Seedy）
scripts/                   # 开发/构建脚本（_env / dev / tauri-dev / build / install）
pet/                       # 外部宠物 zip 素材（开发用，可导入测试）
```

## 外部宠物包格式

本地导入（`.zip`）需包含：

- `pet.json`：`{ "id", "displayName", "description", "spritesheetPath" }`
- `spritesheet.webp`：精灵图

**帧布局**：默认套用 Codex Pet V2 标准（单格 192×208，8 列，11 行）：

- `idle` = row0 / 6 帧；`talk` = row3 / 4 帧
- `actions`：`wave`(row3) / `jump`(row4) / `failed`(row5) / `waiting`(row6) / `working`(row7) / `look`(row8/9) / `runningLeft`(row2) / `runningRight`(row1)

若 `pet.json` 额外提供 `idle` / `talk` / `actions` 字段则覆盖默认值。

**自动越界修正**：不同包的行数不统一（标准 11 行，部分 9 行）。导入时用零依赖的 webp 头解析（VP8 / VP8L / VP8X 三种编码的尺寸都在头部）算出真实行数——`row` 越界则该动作被移除，`count` 越界则截断到该行可用列数，避免宠物因帧越界而画布清空消失。精灵图以 base64 data URL 返回前端，无需额外 asset 协议配置。

## 在线画廊

设置窗口「在线画廊」接入 [awesome-codex-pet](https://github.com/legeling/awesome-codex-pet)：

- 浏览（`browse_online_pets`）：拉取 `pets.json` 索引，显示名优先中文（`localized_names.zh`），预览图来自 `https://codexpet.top/assets/previews/<slug>/webp/idle.webp`（404 时前端回退文字）
- 下载（`download_online_pet`）：拉取 `<slug>/pet.json` + `spritesheet.webp`，复用 `build_pet_def` 组装并落盘到 `app_data_dir/pets/<slug>/`，随后刷新托盘菜单

## 技术栈

- Tauri 2（Rust，零额外网络依赖：`TcpListener` 手写 HTTP、`reqwest` 仅用 rustls 拉取在线索引）
- Vue 3 + TypeScript + Vite
- 跨平台穿透：macOS 用 `objc2`（NSTimer + `NSWindow.setIgnoresMouseEvents`）；Windows 用 `windows-sys`（SetWindowRgn + Per-Monitor DPI）
- 开机自启使用 Apple 官方 `SMAppService`（登录项，无第三方插件依赖）
- 外部宠物包解压用 `zip`（deflate）

## 平台支持

| 功能 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| 透明无边框窗口 + 穿透 | ✅（NSTimer 动态切换） | ✅（SetWindowRgn 裁切） | ⚠️ 配置已就位，未经实测 |
| 开机自启 | ✅（SMAppService 登录项） | ✅（Startup 文件夹 `.lnk` 快捷方式） | ❌ |
| 在线画廊 / 外部导入 | ✅ | ✅ | ✅ |
| 通知 HTTP 接口 | ✅ | ✅ | ✅ |

> 打包前会自动重新生成托盘图标（见 `scripts/`，无需手动处理）。
