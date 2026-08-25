#!/bin/bash
# PetBuddy 依赖安装脚本
# 使用 Homebrew 的 node@20 LTS 运行（避免 node 26 导致 rollup 原生二进制损坏）

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

# 加载公共环境（node@20）
source "$SCRIPT_DIR/_env.sh"

echo "使用 Node: $(node -v) / npm: $(npm -v)"
echo "安装目录: $PROJECT_DIR"
echo ""

# 安装依赖（透传所有参数，如 --legacy-peer-deps）
exec npm install "$@"
