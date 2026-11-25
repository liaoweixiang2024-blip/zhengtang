#!/usr/bin/env bash
# zhengtang-agent 快捷管理（双平台：Linux=systemd / macOS=launchd）
# 独立分发的最新版——已装机器可直接覆盖更新本命令：
#   curl -fsSL <raw 地址>/scripts/zt-agent.sh | sudo tee /usr/local/bin/zt-agent >/dev/null
# ⚠ 与 scripts/install.sh 末尾内嵌生成的 zt-agent 内容保持一致，改一处记得同步另一处
set -euo pipefail
CMD="${1:-}"

# 路径：Linux 与 macOS 的服务配置不同，二进制/包路径一致
if [ "$(uname -s)" = "Darwin" ]; then
  IS_MAC=1
  SERVICE="ai.zhengtang.agent"
  PLIST="/Library/LaunchDaemons/$SERVICE.plist"
  CONF="/usr/local/etc/zhengtang/agent.toml"
  LOG="/Library/Logs/zhengtang-agent.log"
else
  IS_MAC=0
  CONF="/etc/zhengtang/agent.toml"
fi

# 特权命令（动服务 / 写系统目录）非 root 自动 sudo 重跑。
# 普通用户直接 systemctl restart 会走 polkit 授权，而部分 NAS 系统
# （如绿联 UGOS）的 polkit-agent-helper 缺 setuid，授权必失败——sudo 是唯一通路。
case "$CMD" in
  status|log|logs|restart|stop|start|upgrade|uninstall)
    if [ "$(id -u)" != 0 ]; then
      if command -v sudo >/dev/null 2>&1; then
        if [ -n "${ZT_BASE_URL:-}" ]; then
          exec sudo ZT_BASE_URL="$ZT_BASE_URL" ${ZT_MIRROR:+ZT_MIRROR="$ZT_MIRROR"} "$0" "$@"
        elif [ -n "${ZT_MIRROR:-}" ]; then
          exec sudo ZT_MIRROR="$ZT_MIRROR" "$0" "$@"
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

svc_restart() {
  if [ "$IS_MAC" = 1 ]; then
    launchctl kickstart -k "system/$SERVICE"
  else
    systemctl restart zhengtang-agent
  fi
}
svc_stop() {
  if [ "$IS_MAC" = 1 ]; then
    launchctl bootout "system/$SERVICE" 2>/dev/null || true
  else
    systemctl stop zhengtang-agent
  fi
}
svc_start() {
  if [ "$IS_MAC" = 1 ]; then
    launchctl bootstrap system "$PLIST" 2>/dev/null || launchctl load -w "$PLIST"
  else
    systemctl start zhengtang-agent
  fi
}

case "$CMD" in
  status)
    if [ "$IS_MAC" = 1 ]; then
      launchctl print "system/$SERVICE" 2>/dev/null | grep -E "state|pid|last exit" | head -5 \
        || echo "未运行（launchd 中无此服务）"
    else
      systemctl status zhengtang-agent --no-pager
    fi ;;
  log|logs)
    if [ "$IS_MAC" = 1 ]; then
      tail -f "$LOG"
    else
      journalctl -u zhengtang-agent -f
    fi ;;
  restart) svc_restart && echo "✓ 已重启" ;;
  stop)    svc_stop && echo "✓ 已停止" ;;
  start)   svc_start && echo "✓ 已启动" ;;
  upgrade)
    VER="${2:-latest}"
    OFFICIAL="https://github.com/liaoweixiang2024-blip/zhengtang/releases"
    MIRRORS="${ZT_MIRROR:-https://ghfast.top/$OFFICIAL https://gh-proxy.com/$OFFICIAL}"
    OS="$(uname -s)"; ARCH="$(uname -m)"
    case "$OS-$ARCH" in
      Linux-x86_64)                TARGET="x86_64-unknown-linux-musl" ;;
      Linux-aarch64|Linux-arm64)   TARGET="aarch64-unknown-linux-musl" ;;
      Linux-armv7l|Linux-armv7)    TARGET="armv7-unknown-linux-musleabihf" ;;
      Darwin-arm64|Darwin-aarch64) TARGET="aarch64-apple-darwin" ;;
      Darwin-x86_64)               TARGET="x86_64-apple-darwin" ;;
      *) echo "不支持的架构: $OS $ARCH"; exit 1 ;;
    esac
    TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
    # 源顺序：显式 ZT_BASE_URL 只用之；否则官方 4s 探测——可达用「官方+镜像兜底」，
    # 不可达（国内常态）直接镜像。latest 在镜像 CDN 可能回旧缓存，显式版本号可绕过。
    if [ -n "${ZT_BASE_URL:-}" ]; then
      BASES="$ZT_BASE_URL"
    elif curl -m 4 -fsI -o /dev/null "$OFFICIAL" 2>/dev/null; then
      BASES="$OFFICIAL $MIRRORS"
    else
      BASES="$MIRRORS"
      echo "官方 GitHub 不可达，自动走镜像"
    fi
    PKG="zhengtang-agent-$TARGET.tar.gz"
    OK=""
    for B in $BASES; do
      DL="$B/latest/download"; [ "$VER" != "latest" ] && DL="$B/download/$VER"
      echo "尝试源: $B"
      RC=0
      curl -fsSL --connect-timeout 6 -m 300 "$DL/$PKG" -o "$TMP/a.tgz" || RC=$?
      if [ "$RC" = "23" ]; then
        # 23=写盘失败：换源也无济于事，直接给出可操作提示
        # （不能写 `if ! curl; then [ $? = 23 ]`——`!` 会把退出码翻成 0）
        echo "本地写盘失败（磁盘满？检查 /tmp 所在分区空间）"; exit 1
      fi
      [ "$RC" = "0" ] || continue
      # sha256 同源校验（校验文件拉不到则跳过校验，不因此阻断）
      if curl -fsSL --connect-timeout 6 -m 60 "$DL/$PKG.sha256" -o "$TMP/a.sha256" 2>/dev/null && [ -s "$TMP/a.sha256" ]; then
        EXPECT=$(awk '{print $1}' "$TMP/a.sha256")
        if [ -n "$EXPECT" ] && [ "$(sha256sum "$TMP/a.tgz" | awk '{print $1}')" != "$EXPECT" ]; then
          echo "sha256 校验失败，换下一源"; continue
        fi
      fi
      OK="$B"; break
    done
    [ -n "$OK" ] || { echo "所有源均下载失败（官方与镜像都不可达？）"; exit 1; }
    tar -xzf "$TMP/a.tgz" -C "$TMP"
    UPGRADE_BIN=$(find "$TMP" -type f -name zhengtang-agent | head -1)
    [ -n "$UPGRADE_BIN" ] || { echo "包内未找到 zhengtang-agent"; exit 1; }
    install -m 0755 "$UPGRADE_BIN" /usr/local/bin/zhengtang-agent
    svc_restart
    # 顺手自刷新 zt-agent 命令本身（v0.3.8+ 的包内已带；mv 换 inode，
    # 不影响正在运行的脚本收尾）
    ZT_CMD=$(find "$TMP" -type f -name zt-agent | head -1)
    if [ -n "$ZT_CMD" ]; then
      if cp "$ZT_CMD" /usr/local/bin/zt-agent.new && chmod 755 /usr/local/bin/zt-agent.new && mv /usr/local/bin/zt-agent.new /usr/local/bin/zt-agent; then
        echo "✓ zt-agent 命令已同步更新"
      else
        echo "（zt-agent 命令未刷新，不影响本次升级）"
      fi
    fi
    echo "✓ 已升级到 ${VER} 并重启"
    ;;
  uninstall)
    echo "将停止并删除 zhengtang-agent（含 agent.toml 私钥配置）"
    read -r -p "确定? 输入 yes 确认: " ok
    [ "$ok" = "yes" ] || { echo "已取消"; exit 0; }
    if [ "$IS_MAC" = 1 ]; then
      svc_stop
      rm -f "$PLIST" /usr/local/bin/zhengtang-agent /usr/local/bin/zt-agent
      rm -rf /usr/local/etc/zhengtang
    else
      systemctl disable --now zhengtang-agent || true
      rm -f /etc/systemd/system/zhengtang-agent.service /usr/local/bin/zhengtang-agent /usr/local/bin/zt-agent
      rm -rf /etc/zhengtang
      systemctl daemon-reload
    fi
    echo "✓ 已卸载（面板上该机器会显示离线；如需彻底清除请到后台删除该服务器）"
    ;;
  *)
    cat <<'USAGE'
zt-agent — zhengtang 探针管理（Linux/macOS）
  status     查看运行状态
  log        跟随实时日志
  start|stop|restart  启停/重启
  upgrade [版本]  升级并重启（默认 latest；可指定如 v0.3.8）
  uninstall  卸载（删除服务/二进制/配置，需确认）
USAGE
    ;;
esac
