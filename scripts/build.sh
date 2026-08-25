#!/bin/bash
# PetBuddy 生产构建脚本
# 使用 Homebrew 的 node@20 LTS 运行

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

# 加载公共环境（node@20）
source "$SCRIPT_DIR/_env.sh"

echo "使用 Node: $(node -v) / npm: $(npm -v)"
echo "构建目录: $PROJECT_DIR"
echo ""

# 生产构建：前端 vite build + Tauri 打包（生成 .app / .dmg 等产物）
npm run build "$@"

echo ""
echo "前端构建完成，开始 Tauri 打包…"
npm run tauri build
