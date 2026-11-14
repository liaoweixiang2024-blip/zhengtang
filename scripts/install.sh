#!/usr/bin/env bash
# zhengtang-agent 一键安装（curl | sh）
# 用法:
#   curl -fsSL https://<server>/install.sh | sh -s -- \
#     --server wss://<server>:8800/ws/agent \
#     --id <agent_id> --key <hex> --fingerprint <hex>
#
# 兼容性：curl | sh 时 shebang 不生效，脚本可能被 dash（Ubuntu 默认 sh）执行
# ——pipefail 是 bash 专属，能力探测开启，dash 下静默跳过
set -eu
(set -o pipefail) 2>/dev/null && set -o pipefail

# ---------- 参数 ----------
# 默认取面板注入的 ZT_*（短安装命令 /a/<token> 场景）；命令行参数可覆盖
SERVER="${ZT_SERVER:-}" ID="${ZT_ID:-}" KEY="${ZT_KEY:-}" FINGERPRINT="${ZT_FP:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --server) SERVER="$2"; shift 2 ;;
    --id) ID="$2"; shift 2 ;;
    --key) KEY="$2"; shift 2 ;;
    --fingerprint) FINGERPRINT="$2"; shift 2 ;;
    *) echo "未知参数: $1"; exit 1 ;;
  esac
done
[ -n "$SERVER" ] && [ -n "$ID" ] && [ -n "$KEY" ] || {
  echo "缺少连接参数：请使用面板生成的安装命令，或传 --server / --id / --key"; exit 1
}

# ---------- 探测架构 ----------
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-musl" ;;
  armv7l|armv7) TARGET="armv7-unknown-linux-musleabihf" ;;
  *) echo "不支持的架构: $ARCH"; exit 1 ;;
esac
[ "$(uname -s)" = "Linux" ] || { echo "此脚本仅支持 Linux（macOS/Windows 请手动下载）"; exit 1; }
# 写 /usr/local/bin 与 systemd 需要 root
[ "$(id -u)" = 0 ] || { echo "请用 root 运行（curl ... | sudo sh）"; exit 1; }

# ---------- 下载 ----------
VERSION="${ZT_VERSION:-latest}"
BASE="${ZT_BASE_URL:-https://github.com/liaoweixiang2024-blip/zhengtang/releases}"
if [ -n "${ZT_DL_BASE:-}" ]; then
  # 面板自托管（data/agents/）：无需访问 GitHub
  URL="${ZT_DL_BASE%/}/zhengtang-agent-$TARGET.tar.gz"
elif [ "$VERSION" = "latest" ]; then
  URL="$BASE/latest/download/zhengtang-agent-$TARGET.tar.gz"
else
  URL="$BASE/download/$VERSION/zhengtang-agent-$TARGET.tar.gz"
fi
echo "下载 $URL"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
CURL_FLAGS="-fsSL"
case "$URL" in https://*) [ -n "${ZT_DL_BASE:-}" ] && CURL_FLAGS="-kfsSL" ;; esac
curl $CURL_FLAGS "$URL" -o "$TMP/agent.tar.gz"
tar -xzf "$TMP/agent.tar.gz" -C "$TMP"

# ---------- 安装 ----------
# 兼容两种打包布局：平铺 或 <name>-<target>/ 子目录
AGENT_BIN=$(find "$TMP" -type f -name zhengtang-agent | head -1)
[ -n "$AGENT_BIN" ] || { echo "包内未找到 zhengtang-agent"; exit 1; }
install -m 0755 "$AGENT_BIN" /usr/local/bin/zhengtang-agent
# 专用低权用户（DynamicUser 的动态 uid 读不了 0600 配置，导致启动即退）
id zhengtang-agent >/dev/null 2>&1 || useradd -r -s /usr/sbin/nologin zhengtang-agent
mkdir -p /etc/zhengtang
chmod 0755 /etc/zhengtang
# fingerprint 行预组装：${var:+...} 内的引号会被 bash 吃掉，裸值会被 TOML 当数字
FP_LINE=""
[ -n "$FINGERPRINT" ] && FP_LINE="fingerprint = \"$FINGERPRINT\""
cat > /etc/zhengtang/agent.toml <<EOF
server = "$SERVER"
id = "$ID"
key = "$KEY"
$FP_LINE
EOF
chown zhengtang-agent:zhengtang-agent /etc/zhengtang/agent.toml
chmod 0600 /etc/zhengtang/agent.toml

# ---------- systemd ----------
# 非 root、无任何特权（严格只读探针不需要 root；进程监控对他 uid 可见性受限）
cat > /etc/systemd/system/zhengtang-agent.service <<'EOF'
[Unit]
Description=zhengtang agent (read-only probe)
After=network-online.target
Wants=network-online.target

[Service]
User=zhengtang-agent
ExecStart=/usr/local/bin/zhengtang-agent --config /etc/zhengtang/agent.toml
Restart=always
RestartSec=5
# 加固
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
MemoryDenyWriteExecute=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
# 只读探针：无写入需求
ReadWritePaths=

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable --now zhengtang-agent
# 覆盖升级场景：enable --now 对已运行服务不重启，强制切到新二进制
systemctl restart zhengtang-agent

# ---------- 快捷管理命令 zt-agent ----------
# 装好后即可用：zt-agent status|log|restart|stop|start|upgrade|uninstall
# ⚠ 与 scripts/zt-agent.sh 内容保持一致（该文件供已装机器单独更新本命令用）
cat > /usr/local/bin/zt-agent <<'ZTEOF'
#!/usr/bin/env bash
# zhengtang-agent 快捷管理
set -euo pipefail
BASE_URL="${ZT_BASE_URL:-https://github.com/liaoweixiang2024-blip/zhengtang/releases}"
CMD="${1:-}"

# 特权命令（动 systemd / 写 /usr/local/bin）非 root 自动 sudo 重跑。
# 普通用户直接 systemctl restart 会走 polkit 授权，而部分 NAS 系统
# （如绿联 UGOS）的 polkit-agent-helper 缺 setuid，授权必失败——sudo 是唯一通路。
case "$CMD" in
  restart|stop|start|upgrade|uninstall)
    if [ "$(id -u)" != 0 ]; then
      if command -v sudo >/dev/null 2>&1; then
        if [ -n "${ZT_BASE_URL:-}" ]; then
          exec sudo ZT_BASE_URL="$ZT_BASE_URL" "$0" "$@"
        else
          exec sudo "$0" "$@"
        fi
      else
        echo "此操作需要 root（当前是普通用户且系统无 sudo）" >&2
        echo "请用: su -c '$0 $*' root" >&2
        exit 1
      fi
    fi
    ;;
esac

case "$CMD" in
  status)  systemctl status zhengtang-agent --no-pager ;;
  log|logs) journalctl -u zhengtang-agent -f ;;
  restart) systemctl restart zhengtang-agent && echo "✓ 已重启" ;;
  stop)    systemctl stop zhengtang-agent && echo "✓ 已停止" ;;
  start)   systemctl start zhengtang-agent && echo "✓ 已启动" ;;
  upgrade)
    VER="${2:-latest}"
    # latest 在镜像 CDN（如 ghfast）可能回旧缓存；显式版本号 zt-agent upgrade v0.2.1 可绕过
    if [ "$VER" = "latest" ]; then DL="$BASE_URL/latest/download"; else DL="$BASE_URL/download/$VER"; fi
    ARCH=$(uname -m)
    case "$ARCH" in
      x86_64)       TARGET="x86_64-unknown-linux-musl" ;;
      aarch64|arm64) TARGET="aarch64-unknown-linux-musl" ;;
      armv7l|armv7)  TARGET="armv7-unknown-linux-musleabihf" ;;
      *) echo "不支持的架构: $ARCH"; exit 1 ;;
    esac
    TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
    echo "下载 $DL/zhengtang-agent-$TARGET.tar.gz"
    curl -fsSL "$DL/zhengtang-agent-$TARGET.tar.gz" -o "$TMP/a.tgz"
    tar -xzf "$TMP/a.tgz" -C "$TMP"
    UPGRADE_BIN=$(find "$TMP" -type f -name zhengtang-agent | head -1)
    [ -n "$UPGRADE_BIN" ] || { echo "包内未找到 zhengtang-agent"; exit 1; }
    install -m 0755 "$UPGRADE_BIN" /usr/local/bin/zhengtang-agent
    systemctl restart zhengtang-agent
    echo "✓ 已升级到 ${VER} 并重启"
    ;;
  uninstall)
    echo "将停止并删除 zhengtang-agent（含 /etc/zhengtang/agent.toml 私钥配置）"
    read -r -p "确定? 输入 yes 确认: " ok
    [ "$ok" = "yes" ] || { echo "已取消"; exit 0; }
    systemctl disable --now zhengtang-agent || true
    rm -f /etc/systemd/system/zhengtang-agent.service /usr/local/bin/zhengtang-agent /usr/local/bin/zt-agent
    rm -rf /etc/zhengtang
    systemctl daemon-reload
    echo "✓ 已卸载（面板上该机器会显示离线；如需彻底清除请到后台删除该服务器）"
    ;;
  *)
    cat <<'USAGE'
zt-agent — zhengtang 探针管理
  status     查看运行状态
  log        跟随实时日志
  start|stop|restart  启停/重启
  upgrade [版本]  升级并重启（默认 latest；可指定如 v0.2.1）
  uninstall  卸载（删除服务/二进制/配置，需确认）
USAGE
    ;;
esac
ZTEOF
chmod 0755 /usr/local/bin/zt-agent

echo "✓ zhengtang-agent 已安装并启动（systemd 单元含沙箱加固）"
echo "  日志: journalctl -u zhengtang-agent -f"
echo "  快捷命令: zt-agent status | log | restart | upgrade | uninstall"
