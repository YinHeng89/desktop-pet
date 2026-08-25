#!/bin/bash
# 公共环境设置：切换到 Homebrew 的 node@20 LTS
# 供 scripts/ 下其它脚本 source 使用

# node@20 的 bin 路径
NODE20_BIN="/opt/homebrew/opt/node@20/bin"

# 若存在则优先使用，否则回退到系统默认 node
if [ -d "$NODE20_BIN" ]; then
  export PATH="$NODE20_BIN:$PATH"
fi
