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

                // 月度流量累计（账期按该机流量重置日对齐）
                let reset_day: Option<i64> = c
                    .query_row(
                        "SELECT traffic_reset_day FROM servers WHERE id = ?1",
                        [id],
                        |r| r.get(0),
                    )
                    .ok()
                    .flatten();
                let period = billing_period(ts, reset_day);
                c.execute(
                    "INSERT INTO traffic_monthly (server_id, month, bytes_in, bytes_out)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(server_id, month)
                     DO UPDATE SET bytes_in = bytes_in + ?3, bytes_out = bytes_out + ?4",
                    rusqlite::params![id, period, di as i64, dou as i64],
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
    // unix 秒 → 'YYYY-MM'（自然月，UTC）
    let (y, m) = year_month_from_days(ts / 86400);
    format!("{y:04}-{m:02}")
}

/// 账期 key：流量统计归入哪个周期。
/// - reset_day=None：自然月，key='YYYY-MM'
/// - reset_day=d(1-31)：按月对齐到 d 号的账期，key='YYYY-MM-DD'（当期起始日）；
///   d 超过当月天数时取当月最后一天（信用卡式惯例：31 号重置 → 2 月按 2/28、4 月按 4/30）
/// - reset_day=99：每月月末哨兵（自动跟随当月最后一天）
///   例：d=15，3-20 → 期起点 3-15（key 2026-03-15）；3-10 → 期起点 2-15（key 2026-02-15）
pub fn billing_period(ts: i64, reset_day: Option<i64>) -> String {
    let Some(d) = reset_day else { return month_string(ts) };
    // 99 用 u32::MAX 表示，min(当月天数) 自然落到月末
    let d = match d {
        99 => u32::MAX,
        d if (1..=31).contains(&d) => d as u32,
        _ => 1,
    };

    // days → (y, m, day) 民用日期
    let mut days = ts / 86400;
    let mut y = 1970i64;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let ydays = if leap { 366 } else { 365 };
        if days >= ydays {
            days -= ydays;
            y += 1;
        } else {
            break;
        }
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 1u32;
    let mut day = days as u32 + 1;
    for &md in &mdays {
        if day <= md {
            break
        }
        day -= md;
        m += 1;
    }

    let cur_start = d.min(mdays[(m - 1) as usize]);
    if day >= cur_start {
        format!("{y:04}-{m:02}-{cur_start:02}")
    } else {
        // 归属上月账期：起点 = 上月 min(d, 上月天数)
        let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, (m - 1) as i64) };
        let pm_days = mdays[(if m == 1 { 12 } else { m - 1 }) as usize - 1];
        let p_start = d.min(pm_days);
        format!("{py:04}-{pm:02}-{p_start:02}")
    }
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
    fn billing_period_natural_month() {
        assert_eq!(billing_period(1794384000, None), "2026-11");
    }

    #[test]
    fn billing_period_anchor_day() {
        // 2026-11-12，reset=15 → 归 2026-10-15 期
        let ts_1112 = day_ts(2026, 11, 12);
        assert_eq!(billing_period(ts_1112, Some(15)), "2026-10-15");
        // 2026-11-20 → 归 2026-11-15 期
        let ts_1120 = day_ts(2026, 11, 20);
        assert_eq!(billing_period(ts_1120, Some(15)), "2026-11-15");
        // 跨年：2027-01-05 reset=15 → 2026-12-15 期
        let ts_0105 = day_ts(2027, 1, 5);
        assert_eq!(billing_period(ts_0105, Some(15)), "2026-12-15");
    }

    #[test]
    fn billing_period_month_end_semantics() {
        // 31 号重置：大月照常，小月/2 月收缩到当月最后一天
        assert_eq!(billing_period(day_ts(2026, 11, 20), Some(31)), "2026-10-31"); // 11 月收缩日是 11-30，20 号归上期（10-31）
        assert_eq!(billing_period(day_ts(2026, 12, 5), Some(31)), "2026-11-30"); // 12 月初 → 上期 11 月收缩到 30
        assert_eq!(billing_period(day_ts(2026, 3, 5), Some(31)), "2026-02-28"); // 3 月初 → 上期 2 月末
        assert_eq!(billing_period(day_ts(2026, 4, 30), Some(31)), "2026-04-30"); // 4 月 30 = 当月收缩日当天
        assert_eq!(billing_period(day_ts(2028, 3, 1), Some(29)), "2028-02-29"); // 闰年 2 月 29 存在
        assert_eq!(billing_period(day_ts(2026, 3, 1), Some(29)), "2026-02-28"); // 平年收缩到 2-28
        // 99 = 月末哨兵：每月自动跟最后一天
        assert_eq!(billing_period(day_ts(2026, 2, 10), Some(99)), "2026-01-31");
        assert_eq!(billing_period(day_ts(2026, 4, 15), Some(99)), "2026-03-31");
        assert_eq!(billing_period(day_ts(2026, 12, 31), Some(99)), "2026-12-31");
    }

    fn day_ts(y: i64, m: u32, d: u32) -> i64 {
        // 复用民用历换算：直接调 billing_period 内部逻辑太绕，用秒级近似构造
        // 1970 起步累加
        let mut days = 0i64;
        for yy in 1970..y {
            let leap = (yy % 4 == 0 && yy % 100 != 0) || yy % 400 == 0;
            days += if leap { 366 } else { 365 };
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (i, &md) in mdays.iter().enumerate() {
            if (i as u32) + 1 >= m {
                break
            }
            days += md as i64;
        }
        days += d as i64 - 1;
        days * 86400 + 43200 // 当天正午，避免时区边界
    }

    #[test]
    fn month_string_correct() {
        assert_eq!(month_string(0), "1970-01");
        assert_eq!(month_string(1735689600), "2025-01"); // 2025-01-01
        assert_eq!(month_string(1794384000), "2026-11"); // 2026-11 中旬
    }
}
