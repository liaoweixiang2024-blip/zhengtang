#!/usr/bin/env bash
# zhengtang-server 安装（Linux systemd）
set -euo pipefail

ARCH=$(uname -m)
case "$ARCH" in
  x86_64) TARGET="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-musl" ;;
  *) echo "不支持的架构: $ARCH"; exit 1 ;;
esac
[ "$(uname -s)" = "Linux" ] || { echo "此脚本仅支持 Linux"; exit 1; }

VERSION="${ZT_VERSION:-latest}"
BASE="${ZT_BASE_URL:-https://github.com/liaoweixiang2024-blip/zhengtang/releases}"
if [ "$VERSION" = "latest" ]; then
  URL="$BASE/latest/download/zhengtang-server-$TARGET.tar.gz"
else
  URL="$BASE/download/$VERSION/zhengtang-server-$TARGET.tar.gz"
fi
echo "下载 $URL"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
curl -fsSL "$URL" -o "$TMP/server.tar.gz"
tar -xzf "$TMP/server.tar.gz" -C "$TMP"

# 兼容两种打包布局：平铺 或 <name>-<target>/ 子目录
SERVER_BIN=$(find "$TMP" -type f -name zhengtang-server | head -1)
[ -n "$SERVER_BIN" ] || { echo "包内未找到 zhengtang-server"; exit 1; }
install -m 0755 "$SERVER_BIN" /usr/local/bin/zhengtang-server
mkdir -p /var/lib/zhengtang

cat > /etc/systemd/system/zhengtang-server.service <<'EOF'
[Unit]
Description=zhengtang server
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/zhengtang-server --data-dir /var/lib/zhengtang serve
Restart=always
RestartSec=5
WorkingDirectory=/var/lib/zhengtang
StateDirectory=zhengtang
NoNewPrivileges=yes
ProtectHome=yes
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
EOF

# systemctl daemon-reload
# systemctl enable --now zhengtang-server
# 覆盖升级场景：enable --now 对已运行服务不重启，强制切到新二进制
# systemctl restart zhengtang-server
echo "✓ zhengtang-server 已启动: https://<本机IP>:8800"
echo "  TLS 指纹: journalctl -u zhengtang-server | grep 指纹"
echo "  首次使用请打开面板完成管理员创建"
