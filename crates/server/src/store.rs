//! SQLite 存储层：rusqlite（bundled）+ `PRAGMA user_version` 迁移。
//!
//! 单进程单机场景，用 `Mutex<Connection>` 短临界区即可；实时指标走内存
//! （见 state.rs），DB 只承接低频读写（注册表/聚合样本/审计）。

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::Connection;

const MIGRATIONS: &[(&str, &str)] = &[
    ("001_init", include_str!("../migrations/001_init.sql")),
    ("002_sessions", include_str!("../migrations/002_sessions.sql")),
];

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("打开数据库失败: {}", path.display()))?;
        Self::init(conn).map(|conn| Self { conn: Mutex::new(conn) })
    }

    fn init(conn: Connection) -> Result<Connection> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, (name, sql)) in MIGRATIONS.iter().enumerate() {
            let target = (i + 1) as i64;
            if version < target {
                conn.execute_batch(sql)
                    .with_context(|| format!("执行迁移 {name} 失败"))?;
                conn.pragma_update(None, "user_version", target)?;
                tracing::info!(migration = name, to = target, "数据库迁移完成");
            }
        }
        Ok(conn)
    }

    /// 在锁内执行闭包。操作保持短小（毫秒级）。
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        f(&conn)
    }
}

/// 注册一台被监控服务器（由 add-server 子命令 / 管理 API 调用）。
pub fn insert_server(
    conn: &Connection,
    id: &str,
    name: &str,
    pubkey: &[u8; 32],
) -> Result<()> {
    conn.execute(
        "INSERT INTO servers (id, name, pubkey, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, name, pubkey, unix_now()],
    )?;
    Ok(())
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_on_fresh_db() {
        let dir = tempfile_dir();
        let store = Store::open(&dir).unwrap();
        store
            .with(|c| {
                let n: i64 = c.query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0))?;
                assert_eq!(n, 0);
                let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                assert_eq!(v, MIGRATIONS.len() as i64);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn migrations_idempotent() {
        let dir = tempfile_dir();
        let _ = Store::open(&dir).unwrap();
        let _ = Store::open(&dir).unwrap(); // 二次打开不应重复执行或报错
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("zt-test-{}.db", uuid::Uuid::new_v4()));
        p
    }
}
