#!/usr/bin/env bash
# 协议安全机检：验证 ServerMsg 枚举不含任何执行/文件原语。
# 这是"严格只读"承诺的机器检查，CI 必跑。人为引入违规变体会直接挂 CI。
set -euo pipefail

PROTO="crates/proto/src/lib.rs"

[ -f "$PROTO" ] || { echo "找不到 $PROTO"; exit 1; }

# 提取 ServerMsg 枚举块（从 "enum ServerMsg" 到下一个顶层的 "}" 前的 enum 结束）
BLOCK=$(awk '/pub enum ServerMsg/,/^}/' "$PROTO")
[ -n "$BLOCK" ] || { echo "无法定位 ServerMsg 枚举"; exit 1; }

# 允许的变体白名单（穷举；新增只读能力需在此登记并评审）
ALLOWED="Challenge|HelloOk|HelloErr|Ping|DialTasks|ProcessRules|RequestFullReport"

# 1) 枚举变体机检：每个变体必须在白名单
echo "$BLOCK" | grep -oE '^    [A-Z][A-Za-z]+' | tr -d ' ' | while read -r v; do
  if ! echo "$ALLOWED" | tr '|' '\n' | grep -qx "$v"; then
    echo "✗ ServerMsg 出现未登记的变体: $v"
    echo "  若确属只读能力，请先在 scripts/check-protocol.sh 白名单登记并做安全评审"
    exit 1
  fi
done

# 2) 字段机检：枚举块内禁止出现命令/执行/文件语义的标识符
FORBIDDEN='exec|command|cmd|shell|spawn|process_kill|kill_with|write_file|read_file|upload|download|path|script|eval|bash|sh_command'
if echo "$BLOCK" | grep -inE "$FORBIDDEN" | grep -v 'ProcessRules'; then
  echo "✗ ServerMsg 含疑似执行/文件原语字段（见上）"
  exit 1
fi

# 3) agent 侧禁止引入反向执行通道：agent 不得 std::process::Command
if grep -rn "std::process::Command\|process::Command" crates/agent/src/; then
  echo "✗ agent 源码出现进程执行调用"
  exit 1
fi

echo "✓ 协议机检通过：ServerMsg 穷举只读变体，agent 无执行原语"
