-- 兜底：确保 sessions 表存在（幂等；针对早期 001 版本创建的库）
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    ip TEXT,
    ua TEXT
);
