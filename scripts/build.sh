#!/bin/bash
# PetBuddy 本地生产构建脚本
# 用法：
#   ./scripts/build.sh          自动按小版本(patch)自增，如 0.1.0 -> 0.1.1
#   ./scripts/build.sh 0.1.5    手动指定版本号
# 版本号会同步写回 package.json / src-tauri/tauri.conf.json / src-tauri/Cargo.toml
# （不会自动 git commit，改动需自行提交）

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

# 加载公共环境（node@20）
source "$SCRIPT_DIR/_env.sh"

echo "使用 Node: $(node -v) / npm: $(npm -v)"
echo "构建目录: $PROJECT_DIR"
echo ""

# ── 版本号处理 ──
PKG_FILE="package.json"
CONF_FILE="src-tauri/tauri.conf.json"
CARGO_FILE="src-tauri/Cargo.toml"

INPUT_VERSION="${1:-}"
NEW_VERSION=""

if [ -n "$INPUT_VERSION" ]; then
  # 手动指定：简单校验 semver 三段式
  if ! echo "$INPUT_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "错误：版本号格式应为 X.Y.Z，收到 '$INPUT_VERSION'" >&2
    exit 1
  fi
  NEW_VERSION="$INPUT_VERSION"
  echo "使用手动指定版本: $NEW_VERSION"
else
  # 自动 patch 自增
  CUR_VERSION="$(node -p "require('./$PKG_FILE').version")"
  IFS='.' read -r MAJ MIN PAT <<< "$CUR_VERSION"
  NEW_VERSION="${MAJ}.${MIN}.$((PAT + 1))"
  echo "自动自增版本: $CUR_VERSION -> $NEW_VERSION"
fi

# 写回三处版本文件
node -e '
  const fs = require("fs");
  const pkg = JSON.parse(fs.readFileSync("'"$PKG_FILE"'", "utf8"));
  const conf = JSON.parse(fs.readFileSync("'"$CONF_FILE"'", "utf8"));
  const cargo = fs.readFileSync("'"$CARGO_FILE"'", "utf8");
  const v = "'"$NEW_VERSION"'";
  pkg.version = v;
  conf.version = v;
  const newCargo = cargo.replace(/^version = ".*"$/m, `version = "${v}"`);
  fs.writeFileSync("'"$PKG_FILE"'", JSON.stringify(pkg, null, 2) + "\n");
  fs.writeFileSync("'"$CONF_FILE"'", JSON.stringify(conf, null, 2) + "\n");
  fs.writeFileSync("'"$CARGO_FILE"'", newCargo);
  console.log("已写回 " + v + " 到 package.json / tauri.conf.json / Cargo.toml");
'

echo ""

# 生产构建：前端 vite build + Tauri 打包（生成 .app / .dmg 等产物）
npm run build "$@"

echo ""
echo "前端构建完成，开始 Tauri 打包…"
npm run tauri build

echo ""
echo "构建完成，当前版本: $NEW_VERSION（如有改动请自行 git commit）"
