-- zhengtang 初始 schema（PRAGMA user_version = 1）

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'viewer',        -- 'admin' | 'viewer'
    totp_secret TEXT,                           -- base32，启用 2FA 后非空（加密存储见 M4）
    totp_enabled INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS user_groups (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, group_id)
);

CREATE TABLE IF NOT EXISTS servers (
    id TEXT PRIMARY KEY,                        -- agent_id（uuid）
    name TEXT NOT NULL,
    group_id INTEGER REFERENCES groups(id) ON DELETE SET NULL,
    pubkey BLOB NOT NULL,                       -- Ed25519 公钥 32B
    auth_mode TEXT NOT NULL DEFAULT 'ed25519',
    expires_at INTEGER,                         -- unix 秒，NULL = 永不过期
    public_visible INTEGER NOT NULL DEFAULT 0,
    traffic_quota_bytes INTEGER,                -- 月度流量配额，NULL = 不限
    tags TEXT NOT NULL DEFAULT '[]',            -- JSON array
    last_seen_at INTEGER,
    last_metrics_json TEXT,                     -- 最近一次实时指标（重启后仪表盘仍可见）
    system_info_json TEXT,                      -- 最近一次 FullReport
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS metric_samples (
    server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,                        -- unix 秒（对齐到分钟/小时）
    granularity INTEGER NOT NULL,               -- 1 = 1min, 2 = 1h
    cpu_avg REAL NOT NULL, cpu_min REAL NOT NULL, cpu_max REAL NOT NULL,
    mem_used INTEGER NOT NULL,
    disk_used_json TEXT NOT NULL DEFAULT '{}',
    net_in INTEGER NOT NULL, net_out INTEGER NOT NULL,  -- 周期内字节数
    load1 REAL NOT NULL,
    gpu_json TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (server_id, granularity, ts)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS traffic_monthly (
    server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    month TEXT NOT NULL,                        -- 'YYYY-MM'
    bytes_in INTEGER NOT NULL,
    bytes_out INTEGER NOT NULL,
    PRIMARY KEY (server_id, month)
);

CREATE TABLE IF NOT EXISTS dial_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,                         -- 'http' | 'tcp'
    target TEXT NOT NULL,                       -- URL 或 host:port JSON
    interval_secs INTEGER NOT NULL DEFAULT 60,
    timeout_ms INTEGER NOT NULL DEFAULT 5000,
    enabled INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS dial_results (
    task_id INTEGER NOT NULL REFERENCES dial_tasks(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    ok INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    status TEXT NOT NULL,
    PRIMARY KEY (task_id, ts)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS process_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    exact INTEGER NOT NULL DEFAULT 1,
    display_name TEXT
);

CREATE TABLE IF NOT EXISTS alert_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id TEXT REFERENCES servers(id) ON DELETE CASCADE,  -- NULL = 全局
    type TEXT NOT NULL,                         -- offline|cpu|mem|disk|expiry|dial_fail|traffic
    params TEXT NOT NULL DEFAULT '{}',          -- JSON：阈值/持续/剩余天数等
    duration_secs INTEGER NOT NULL DEFAULT 0,   -- 需持续满足的时长
    silence_secs INTEGER NOT NULL DEFAULT 300,  -- 静默期
    enabled INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS alert_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id INTEGER NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE,
    server_id TEXT NOT NULL,
    fired_at INTEGER NOT NULL,
    resolved_at INTEGER,
    summary TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notify_channels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,                         -- telegram|bark|email|webhook
    name TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER REFERENCES notify_channels(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',     -- pending|sent|failed
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    action TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '{}',
    ip TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_servers_last_seen ON servers(last_seen_at);
CREATE INDEX IF NOT EXISTS idx_dial_results_task ON dial_results(task_id, ts DESC);
CREATE INDEX IF NOT EXISTS idx_alert_events_server ON alert_events(server_id, fired_at DESC);
