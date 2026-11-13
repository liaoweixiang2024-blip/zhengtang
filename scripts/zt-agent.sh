#!/usr/bin/env bash
# zhengtang-agent 快捷管理（独立分发的最新版——已装机器可直接覆盖更新本命令：
#   curl -fsSL <raw 地址>/scripts/zt-agent.sh | sudo tee /usr/local/bin/zt-agent >/dev/null
# ⚠ 与 scripts/install.sh 末尾内嵌生成的 zt-agent 内容保持一致，改一处记得同步另一处）
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
