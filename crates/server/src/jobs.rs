//! 后台任务：1min 聚合落库 / 降采样 / 保留期清理 / 月度流量累计。
//!
//! 写入策略（防写爆）：
//! - 实时态在内存（state.metrics）；DB 每台每分钟 1 行聚合
//! - >7 天的 1min 行降采样为 1h 行；1min 保留 30 天、1h 保留 365 天
//! - 月度流量按分钟差分累计 UPSERT

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use proto::RealtimeMetrics;

use crate::state::AppState;
use crate::store::unix_now;

pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(aggregation_loop(state.clone()));
    tokio::spawn(retention_loop(state));
}

/// 对齐到下一个整分钟。
async fn sleep_to_next_minute() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = 60 - (now.as_secs() % 60);
    tokio::time::sleep(Duration::from_secs(secs.max(1))).await;
}

async fn aggregation_loop(state: Arc<AppState>) {
    // 上次聚合时的流量计数（跨分钟差分；进程重启后首轮用速率近似）
    let mut last_counters: HashMap<String, (u64, u64)> = HashMap::new();
    loop {
        sleep_to_next_minute().await;
        let ts = unix_now() - unix_now() % 60;
        let mut rows: Vec<(String, RealtimeMetrics)> = Vec::new();
        for entry in state.metrics.iter() {
            rows.push((entry.key().clone(), entry.value().clone()));
        }
        let _ = state.store.with(|c| {
            for (id, m) in &rows {
                // 只聚合在线机器（离线机无新数据）
                if !state.live.contains_key(id) {
                    continue;
                }
                // CPU 窗口聚合（min/avg/max）
                let (cmin, cavg, cmax) = state.cpu_stats(id);
                // 流量差分
                let (di, dou) = match last_counters.get(id) {
                    Some((pi, po)) => (
                        m.net_in_total.saturating_sub(*pi),
                        m.net_out_total.saturating_sub(*po),
                    ),
                    None => (
                        (m.net_in_speed as i64 * 60).max(0) as u64,
                        (m.net_out_speed as i64 * 60).max(0) as u64,
                    ),
                };
                last_counters.insert(id.clone(), (m.net_in_total, m.net_out_total));

                let root = m.disks.iter().find(|d| d.mount == "/").or_else(|| m.disks.first());
                let disk_json = serde_json::to_string(&root).unwrap_or_else(|_| "null".into());
                let gpu_json = serde_json::to_string(&m.gpus).unwrap_or_else(|_| "[]".into());

                c.execute(
                    "INSERT OR REPLACE INTO metric_samples
                     (server_id, ts, granularity, cpu_avg, cpu_min, cpu_max, mem_used,
                      disk_used_json, net_in, net_out, load1, gpu_json)
                     VALUES (?1,?2,1,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    rusqlite::params![
                        id, ts, cavg, cmin, cmax, m.mem_used as i64, disk_json, di as i64, dou as i64, m.load1, gpu_json
                    ],
                )?;

                // 月度流量累计
                let month = month_string(ts);
                c.execute(
                    "INSERT INTO traffic_monthly (server_id, month, bytes_in, bytes_out)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(server_id, month)
                     DO UPDATE SET bytes_in = bytes_in + ?3, bytes_out = bytes_out + ?4",
                    rusqlite::params![id, month, di as i64, dou as i64],
                )?;
            }
            Ok(())
        });
        tracing::debug!(servers = rows.len(), ts, "1min 聚合完成");
    }
}

async fn retention_loop(state: Arc<AppState>) {
    loop {
        // 每小时执行
        tokio::time::sleep(Duration::from_secs(3600)).await;
        let now = unix_now();
        let week_ago = now - 7 * 86400;
        let month_ago = now - 30 * 86400;
        let year_ago = now - 365 * 86400;
        let _ = state.store.with(|c| {
            // 1min → 1h 降采样（>7 天）
            c.execute(
                "INSERT OR IGNORE INTO metric_samples
                 (server_id, ts, granularity, cpu_avg, cpu_min, cpu_max, mem_used,
                  disk_used_json, net_in, net_out, load1, gpu_json)
                 SELECT server_id, (ts/3600)*3600, 2,
                        AVG(cpu_avg), MIN(cpu_min), MAX(cpu_max), AVG(mem_used),
                        disk_used_json, SUM(net_in), SUM(net_out), AVG(load1), gpu_json
                 FROM metric_samples WHERE granularity = 1 AND ts < ?1
                 GROUP BY server_id, (ts/3600)*3600",
                rusqlite::params![week_ago],
            )?;
            // 1min 保留 30 天
            c.execute("DELETE FROM metric_samples WHERE granularity = 1 AND ts < ?1", [month_ago])?;
            // 1h 保留 365 天
            c.execute("DELETE FROM metric_samples WHERE granularity = 2 AND ts < ?1", [year_ago])?;
            Ok(())
        });
        tracing::info!("降采样与保留期清理完成");
    }
}

fn month_string(ts: i64) -> String {
    // unix 秒 → 'YYYY-MM'（UTC，月度计费口径按 UTC 对齐即可）
    let days = ts / 86400;
    // 1970-01-01 起算
    let (y, m) = year_month_from_days(days);
    format!("{y:04}-{m:02}")
}

pub fn year_month_from_days(days_since_epoch: i64) -> (i64, i64) {
    // 简化民用历法（不含闰秒，含闰年）
    let mut y = 1970i64;
    let mut d = days_since_epoch;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let ydays = if leap { 366 } else { 365 };
        if d >= ydays {
            d -= ydays;
            y += 1;
        } else {
            break;
        }
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 1;
    for &md in &mdays {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    (y, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_string_correct() {
        assert_eq!(month_string(0), "1970-01");
        assert_eq!(month_string(1735689600), "2025-01"); // 2025-01-01
        assert_eq!(month_string(1794384000), "2026-11"); // 2026-11 中旬
    }
}
