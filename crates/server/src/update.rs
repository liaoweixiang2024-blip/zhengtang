//! 面板在线自更新：下载 release 包 → sha256 校验 → 原子替换自身 → exec 热切换。
//!
//! 设计要点：
//! - run 必须带显式版本号：镜像 CDN 的 latest 会回旧缓存（实测 ghfast），
//!   显式 /download/vX.Y.Z/ 路径可靠；latest 只用于 check 展示
//! - check 用「禁跟随重定向取 302 Location」的技巧解析最新 tag——
//!   不依赖 api.github.com（国内直连不通），镜像直接 200 回源时返回 None，
//!   前端转「手动输入版本号更新」
//! - 替换：拷贝到当前二进制同目录 .new 再 rename（同分区原子；正在运行的
//!   旧 inode 不受影响，Linux/macOS 均可）
//! - exec 自身热切换：PID 不变、systemd 无感知、HTTP 闪断 <2s；
//!   延时 1.5s 执行，给 HTTP 响应留写出时间
//! - tar 解包只取文件名为 zhengtang-server 的条目读进内存，不按包内路径
//!   落盘——无路径穿越面

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::state::AppState;

pub static UPDATING: AtomicBool = AtomicBool::new(false);

pub const DEFAULT_BASE: &str = "https://github.com/liaoweixiang2024-blip/zhengtang/releases";

/// 本机构建 target triple（与 CI release 产物命名一致；非 release 目标不支持）
pub fn build_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH, cfg!(target_env = "musl")) {
        ("linux", "x86_64", true) => Some("x86_64-unknown-linux-musl"),
        ("linux", "aarch64", true) => Some("aarch64-unknown-linux-musl"),
        ("macos", "x86_64", _) => Some("x86_64-apple-darwin"),
        ("macos", "aarch64", _) => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

/// 更新源（settings.update_base 非空则用之——国内可填 ghfast 镜像前缀）
pub fn update_base(state: &AppState) -> String {
    let b = state.settings().update_base;
    if b.is_empty() {
        DEFAULT_BASE.to_string()
    } else {
        b.trim_end_matches('/').to_string()
    }
}

fn client(secs: u64) -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(secs))
        .build()
        .ok()
}

/// 探测最新版本：请求 latest/download（禁跟随），从 302 Location 解析 vTag。
/// 任何一步不合预期都返回 None（调用方引导手动输入版本号，而不是报错）。
pub async fn latest_version(base: &str, target: &str) -> Option<String> {
    let url = format!("{base}/latest/download/zhengtang-server-{target}.tar.gz");
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if resp.status() != reqwest::StatusCode::FOUND {
        return None;
    }
    let loc = resp.headers().get("location")?.to_str().ok()?.to_string();
    // .../releases/download/v0.2.3/zhengtang-server-....tar.gz → v0.2.3
    let tag = loc.split("/download/").nth(1)?.split('/').next()?.to_string();
    valid_version(&tag).then_some(tag)
}

/// 版本串形如 vX.Y.Z
pub fn valid_version(v: &str) -> bool {
    let rest = v.strip_prefix('v').unwrap_or("");
    let mut parts = rest.split('.');
    let mut n = 0;
    for p in parts.by_ref() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        n += 1;
    }
    n == 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_format() {
        assert!(valid_version("v0.2.3"));
        assert!(valid_version("v10.20.30"));
        // 无 v 前缀 / 段数不对 / 非数字，一律拒绝（这些会拼进下载 URL）
        for bad in ["0.2.3", "v0.2", "v0.2.3.4", "v0.2.x", "v..", "", "latest", "v0.2.3-rc1"] {
            assert!(!valid_version(bad), "{bad} 不应通过");
        }
    }

    #[test]
    fn target_triple_present() {
        // 发布目标是 musl-linux 与 macOS；CI 的 test job 跑在 linux-gnu（开发口径）
        // ——按设计返回 None（不支持在线更新，避免下载错包），两边都验证到
        if cfg!(target_os = "macos") || (cfg!(target_os = "linux") && cfg!(target_env = "musl")) {
            assert!(build_target().is_some(), "release 目标平台必须支持在线更新");
        } else {
            assert!(build_target().is_none(), "非发布目标应返回 None 而非匹配到错误三元组");
        }
    }
}

/// 下载 + sha256 校验 + 解包，返回暂存的二进制路径（data_dir/update/…）。
/// 不做替换——把替换与 exec 留给调用方在响应写出之后执行。
pub async fn download(state: &AppState, version: &str) -> Result<PathBuf, String> {
    let target = build_target().ok_or_else(|| "当前平台不支持在线更新（请用安装脚本升级）".to_string())?;
    if !valid_version(version) {
        return Err("版本号格式应为 vX.Y.Z（如 v0.2.3）".into());
    }
    let base = update_base(state);
    let file = format!("zhengtang-server-{target}.tar.gz");
    let url = format!("{base}/download/{version}/{file}");

    let client = client(120).ok_or("构建 HTTP 客户端失败")?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}（检查更新源是否可达，可在站点设置里填镜像前缀）"))?;
    let resp = resp.error_for_status().map_err(|e| format!("下载失败：{e}"))?;
    let bytes = resp.bytes().await.map_err(|e| format!("下载中断：{e}"))?;

    // sha256 校验（.sha256 与包同源下载，防传输损坏/半包）
    let sha_url = format!("{url}.sha256");
    let sha_resp = client
        .get(&sha_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "sha256 校验文件下载失败".to_string())?;
    let sha_text = sha_resp.text().await.map_err(|_| "sha256 校验文件下载失败".to_string())?;
    let expect = sha_text
        .split_whitespace()
        .next()
        .ok_or("sha256 文件为空")?
        .to_string();
    let got = data_encoding::HEXLOWER.encode(&Sha256::digest(&bytes));
    if !expect.eq_ignore_ascii_case(&got) {
        return Err("sha256 校验失败（下载不完整或源不可信，已中止）".into());
    }

    // 解包：只认文件名为 zhengtang-server 的条目（无穿越面）
    let decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(decoder);
    let mut bin: Option<Vec<u8>> = None;
    for entry in archive.entries().map_err(|e| format!("包损坏：{e}"))? {
        let mut e = entry.map_err(|e| format!("包损坏：{e}"))?;
        let name = e.path().ok().and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));
        if name.as_deref() == Some("zhengtang-server") {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut buf).map_err(|e| format!("解包失败：{e}"))?;
            bin = Some(buf);
            break;
        }
    }
    let bin = bin.ok_or("包内未找到 zhengtang-server")?;

    let dir = state.data_dir.join("update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建暂存目录失败：{e}"))?;
    let out = dir.join(format!("zhengtang-server-{version}"));
    std::fs::write(&out, &bin).map_err(|e| format!("写暂存文件失败：{e}（data 目录磁盘满？）"))?;
    Ok(out)
}

/// 原子替换当前二进制并 exec 热切换。成功则本函数不返回（进程镜像被替换）。
pub fn replace_and_exec(new_bin: &Path) -> Result<(), String> {
    let cur = std::env::current_exe().map_err(|e| format!("定位自身路径失败：{e}"))?;
    let dir = cur.parent().ok_or("无法定位二进制所在目录")?;
    let stage = dir.join("zhengtang-server.staged");
    std::fs::copy(new_bin, &stage).map_err(|e| {
        format!("写入 {} 失败：{e}（需要对该目录的写权限，通常意味着 root 运行）", dir.display())
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置执行权限失败：{e}"))?;
    }
    std::fs::rename(&stage, &cur).map_err(|e| format!("替换二进制失败：{e}"))?;
    tracing::info!(from = %cur.display(), "二进制已替换，exec 热切换");

    // exec 自身：systemd 视角 PID 不变、服务不重启；args 原样带上（--data-dir 等）
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let args: Vec<String> = std::env::args().skip(1).collect();
        let err = std::process::Command::new(&cur).args(&args).exec();
        return Err(format!("exec 失败：{err}（旧二进制仍在运行，可手动 systemctl restart）"));
    }
    #[cfg(not(unix))]
    Err("该平台不支持热切换".into())
}
