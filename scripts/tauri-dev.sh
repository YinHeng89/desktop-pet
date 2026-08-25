#!/bin/bash
# PetBuddy Tauri 桌面版启动脚本（前端 dev + 原生窗口）
# 使用 Homebrew 的 node@20 LTS 运行

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

# 加载公共环境（node@20）
source "$SCRIPT_DIR/_env.sh"

echo "使用 Node: $(node -v) / npm: $(npm -v)"
echo "启动目录: $PROJECT_DIR"
echo ""

# 启动 Tauri 桌面版（透传所有参数）
exec npm run tauri dev "$@"
