# PetBuddy 🐾

跨平台桌面宠物 + 通用通知接收器（基于 Tauri 2 + Vue 3）。

## 功能

- **宠物常驻**：一只卡通宠物常驻桌面右下角，透明无边框窗口
- **随机小动作**：空闲时随机播放挥手/蹦跳/张望/工作/转头等动作
- **交互**：拖动（跑步）、单击（随机动作+搭话台词）、双击（打开设置）、悬停（张望）
- **缩放**：80%~130% 可调，窗口跟随缩放（不重置位置）
- **显示/隐藏**：托盘菜单或设置窗口切换
- **宠物切换**：内置 3 只（Miku/龙神丸/Seedy）+ 外部 zip 导入/删除
- **专属台词**：每只内置宠物有独立性格的搭话气泡
- **像素穿透**：宠物/气泡可交互，透明区域点击穿透（macOS）
- **开机自启**：托盘菜单 + 设置窗口开关（macOS LaunchAgent，带勾选状态）
- **通用通知接口**：本地 HTTP 服务，任意外部应用可发通知（见下文）
- **外部宠物自适应**：导入时解析精灵图实际尺寸，自动修正越界帧，避免异常布局导致宠物消失

## 开发运行

```bash
npm install
bash ./scripts/tauri-dev.sh    # Tauri 桌面版（前端 dev + 原生窗口）
```

其它脚本（均在 `scripts/` 目录）：

```bash
bash ./scripts/dev.sh          # 仅前端 dev（浏览器，无 Tauri 窗口/托盘）
bash ./scripts/build.sh        # 生产构建（前端 build + Tauri 打包成 .app/.dmg）
bash ./scripts/install.sh      # 安装依赖
```

> 脚本会自动加载 Homebrew 的 node@20 LTS，无需手动切换 Node 版本。

## 打包

```bash
bash ./scripts/build.sh
```

产物生成在 `src-tauri/target/release/bundle/`（`.app`、`.dmg`）。

> 打包前会自动重新生成托盘图标（`scripts/gen-tray-icon.sh`），无需手动处理。

## 通知接口（供外部应用调用）

```
POST http://127.0.0.1:8756/notify
Content-Type: application/json

{
  "text": "通知正文",      // 必填
  "action": "wave",        // 可选：宠物动作（wave/jump/failed/waiting/working/look）
  "duration": 4000         // 可选：气泡显示时长(ms)，默认 4000
}
```

示例（curl）：

```bash
curl -X POST http://127.0.0.1:8756/notify \
  -H "Content-Type: application/json" \
  -d '{"text":"新任务：完成周报","action":"wave"}'
```

## 项目结构

```
src/                 # Vue 前端
  components/        # PetHost（宠物宿主）/ SpritePet（精灵播放）/ PetSettings（设置窗口）
  store/             # pet.ts（宠物状态）/ notify.ts（本地通知）
  pets/              # manifest.json（宠物清单）/ dialogues.ts（台词）
src-tauri/           # Rust 后端
  src/
    main.rs          # 入口：窗口 + 托盘 + 命令注册
    macos_pet.rs     # macOS 像素穿透
    pet_import.rs    # 外部宠物导入（含 webp 尺寸解析 + 越界修正）
    autostart.rs     # 开机自启（LaunchAgent）
    notify_server.rs # 本地通知 HTTP 服务
  icons/             # 应用图标 + 托盘图标（tray.png 由脚本生成）
public/pets/         # 内置宠物精灵图资源
scripts/             # 开发/构建脚本（dev/tauri-dev/build/install）
pet/                 # 外部宠物 zip 素材（开发用，可导入测试）
```

## 外部宠物包格式

`.zip` 包需包含：

- `pet.json`：`{ "id", "displayName", "description" }`
- `spritesheet.webp`：精灵图（192x208 单格，8 列，Codex Pet 标准）

**行数说明**：精灵图行数不固定（标准 11 行，部分包 9 行）。导入时会自动解析精灵图实际尺寸并修正帧布局，无需手动指定。越界的动作会被自动移除，宠物不会因帧越界而消失。

## 技术栈

- Tauri 2（Rust）
- Vue 3 + TypeScript + Vite
- 开机自启为自实现（LaunchAgent，无第三方插件依赖）
