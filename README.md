# zhengtang · 严格只读服务器探针

单二进制的服务器监控系统（类似哪吒监控/Komodo），核心承诺：**Agent 严格只读，协议层不存在任何命令执行原语**——即使服务端被完全攻破，被监控机也不会被入侵。

## 特性

- **在线状态 / 资源监控**：CPU（总+每核）、内存、交换、磁盘、负载、网络速率/累计流量、GPU（sysinfo 内建，NVIDIA/Apple 实测）
- **到期管理**：每台服务器可设到期日，卡片倒计时 + 临近告警
- **流量月报**：月度流量累计（UTC 口径），配额进度条 + 超额告警
- **拨测**：从被监控机发起的受约束检查（仅 HTTP GET 禁重定向限 4KB / TCP connect）
- **进程监控**：按进程名（精确/包含）统计实例数/CPU/内存
- **告警**：offline/CPU/内存/磁盘/到期/流量规则，静默与恢复通知；Telegram / Bark / 邮件 / Webhook 四渠道
- **多用户分组**：admin/viewer 角色，viewer 只见授权分组；TOTP 两步验证；登录防爆破；审计日志
- **公开展示页**：可选对外匿名可见的脱敏状态页（`#/public`）
- **历史图表**：1min 聚合 ×30 天，自动降采样 1h ×365 天，详情页 CPU/内存/网络/负载图
- **部署极简**：服务端单二进制 + SQLite（内嵌前端）；agent musl 静态单文件 + `curl | sh` 安装

## 快速开始

### 服务端

```bash
# 方式一：安装脚本（Linux systemd）
curl -fsSL https://github.com/zhengtang/zhengtang/releases/latest/download/install-server.sh | sudo bash

# 方式二：直接运行（任何平台）
./zhengtang-server serve
# → https://0.0.0.0:8800 （自签 TLS，指纹打印在控制台）
# → 浏览器打开完成首启向导（创建管理员）
```

### 添加被监控服务器

面板「管理 → 服务器 → 添加」生成安装命令，在目标机执行：

```bash
curl -fsSL https://<你的服务端>/install.sh | sh -s -- \
  --server wss://<服务端>:8800/ws/agent \
  --id <agent_id> --key <私钥hex> --fingerprint <指纹hex>
```

或手动：

```bash
zhengtang-agent \
  --server wss://host:8800/ws/agent \
  --id <agent_id> --key <私钥> --fingerprint <指纹>
```

### 私有部署的安装脚本分发

服务端可反代 `/install.sh`（放入 `data/` 目录由你的反代/对象存储提供），release 页也带脚本。

## 安全模型

这是本项目存在的理由，逐层说明：

| 层 | 设计 |
|---|---|
| **协议只读（核心）** | `proto::ServerMsg` 枚举穷尽了服务端对 agent 的全部指令：握手、保活、拨测任务、进程匹配规则、请求全量报告。**不存在**命令执行、文件读写、代码加载原语。`scripts/check-protocol.sh` 在 CI 机检（变体白名单 + 禁词扫描 + agent 源码禁 `std::process::Command`） |
| **网络暴露面** | Agent 从不监听任何端口，只主动外连——被监控机零新增攻击面 |
| **传输加密** | rustls（ring，全链路无 openssl）。默认自签证书 + **SHA-256 指纹固定**：安装命令内嵌指纹，中间人无法冒充；也可用 ACME/反代走正常 WebPKI |
| **身份认证** | 每台 agent 独立 Ed25519 密钥对；challenge-response 签名（私钥永不上线）+ 时间窗防重放；服务端只存公钥 |
| **Agent 权限** | systemd 单元 `DynamicUser` + `ProtectSystem=strict` 等全套沙箱，非 root 运行 |
| **拨测约束** | 仅 admin 可配；HTTP 禁重定向、body ≤4KB、超时钳制；TCP 仅 connect——不能当 SSRF 跳板扫内网端口（协议上只能做连通性检查） |
| **面板安全** | argon2id 密码哈希；session HttpOnly/SameSite/Secure；TOTP 2FA；IP+账号维度指数退避防爆破；全管理操作审计 |
| **数据泄露面** | 公开展示页默认关，开启后仅暴露脱敏字段（无 IP/系统信息/到期价）；viewer 按组隔离 |

### 威胁模型小结

- **服务端被攻破**：攻击者能做的是看到指标、伪造面板数据、给 agent 下发拨测/进程匹配任务——无法在被监控机上执行任何代码。
- **被监控机被攻破**：该 agent 私钥泄露，仅影响该台机器身份（admin 撤销重发即可）；无法横向伪造其他 agent。
- **网络中间人**：TLS 指纹固定（自签）或 WebPKI（域名）下均无法截获/篡改。

## 架构

```
Server（单二进制: axum + SQLite + rust-embed 前端）
├─ /ws/agent      Agent 网关（challenge 握手 / 指标接收 / 任务下发）
├─ /ws/dashboard  面板实时推送（按用户组过滤）
├─ /api/*         REST（认证 / 管理 / 历史 / 公开）
└─ 后台任务        1min 聚合 / 降采样 / 30s 告警求值 / 通知分发
        ▲ wss + postcard（自签指纹固定）
Agent（musl 静态: sysinfo 采集 + 受约束拨测）
```

## 开发

```bash
cargo build --workspace && cargo test --workspace
cd frontend && npm install && npm run dev   # Vite 代理到 127.0.0.1:8800
bash scripts/check-protocol.sh              # 协议安全机检
```

## 已知限制

- 非 root agent 对其他 uid 进程的 CPU/内存可见性受限（Linux 行为）；systemd 模板保持默认不提权
- 历史数据为服务端聚合（1min/1h），非原始秒级
- GPU 温度未采集（sysinfo Gpus 无温度字段）；NVIDIA 温度可在 M 后续版本接 NVML

## License

MIT
