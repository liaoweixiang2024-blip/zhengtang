//! 指标采集（sysinfo）。严格只读：不写任何系统状态，不起任何进程。

use anyhow::Result;
use proto::{DiskUsage, GpuInfo, ProcStat, RealtimeMetrics, SystemInfo};
use sysinfo::{
    Disks, Gpus, Networks, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
};

pub struct Collector {
    sys: System,
    networks: Networks,
    disks: Disks,
    gpus: Gpus,
    /// 上次总流量计数（差分求速率）
    last_net: Option<(u64, u64, std::time::Instant)>,
    /// 进程监控规则（由 Server 下发，默认空）
    process_rules: Vec<proto::ProcessRule>,
}

impl Collector {
    pub fn new() -> Result<Self> {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::nothing().with_usage())
                .with_memory(sysinfo::MemoryRefreshKind::everything()),
        )?;
        Ok(Self {
            sys,
            networks: Networks::new_with_refreshed_list()?,
            disks: Disks::new_with_refreshed_list()?,
            gpus: Gpus::new()?,
            last_net: None,
            process_rules: Vec::new(),
        })
    }

    pub fn set_process_rules(&mut self, rules: Vec<proto::ProcessRule>) {
        self.process_rules = rules;
    }

    /// 一次采集快照。内部双采样语义由周期调用保证（CPU 用量基于差分）。
    pub fn snapshot(&mut self) -> RealtimeMetrics {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.networks.refresh(true);
        self.gpus.refresh(true);

        // ---- 网络：累计计数 + 差分速率 ----
        let (in_total, out_total) = self.networks.iter().fold((0u64, 0u64), |(i, o), (_, d)| {
            (i.saturating_add(d.total_received()), o.saturating_add(d.total_transmitted()))
        });
        let (in_speed, out_speed) = match self.last_net {
            Some((pi, po, at)) => {
                let dt = at.elapsed().as_secs_f64().max(0.1);
                // 计数回绕/网卡重置：新值 < 旧值时按 0 起算
                let di = in_total.saturating_sub(pi);
                let dou = out_total.saturating_sub(po);
                ((di as f64 / dt) as u64, (dou as f64 / dt) as u64)
            }
            None => (0, 0),
        };
        self.last_net = Some((in_total, out_total, std::time::Instant::now()));

        // ---- 磁盘（过滤伪文件系统；macOS APFS 只保留 / 一条容器口径） ----
        let disks = self.disk_list();

        // ---- GPU（sysinfo 内建，无 GPU/不可读则空） ----
        let gpus: Vec<GpuInfo> = self
            .gpus
            .iter()
            .map(|g| GpuInfo {
                name: g.model().map(|s| s.to_string()).unwrap_or_else(|| "GPU".into()),
                utilization: g.usage().unwrap_or(0.0),
                // sysinfo GPU 内存单位为 KB
                mem_total: g.total_memory().unwrap_or(0).saturating_mul(1024),
                mem_used: g.used_memory().unwrap_or(0).saturating_mul(1024),
                temperature: -1.0,
            })
            .collect();

        // ---- 进程（仅在有规则时刷新，省 CPU） ----
        let mut processes = Vec::new();
        let mut process_count = 0u64;
        if !self.process_rules.is_empty() {
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );
            process_count = self.sys.processes().len() as u64;
            for rule in &self.process_rules {
                let (count, cpu, mem) = self
                    .sys
                    .processes()
                    .values()
                    .filter(|p| {
                        let name = p.name();
                        if rule.exact {
                            name.as_encoded_bytes() == rule.name.as_bytes()
                        } else {
                            name.to_string_lossy().contains(&rule.name)
                        }
                    })
                    .fold((0u32, 0f32, 0u64), |(c, cp, m), p| {
                        (c + 1, cp + p.cpu_usage(), m.saturating_add(p.memory()))
                    });
                processes.push(ProcStat { rule_id: rule.id, count, cpu, mem_bytes: mem });
            }
        }

        let load = System::load_average().ok();

        RealtimeMetrics {
            cpu_usage: self.sys.global_cpu_usage(),
            cpu_per_core: self.sys.cpus().iter().map(|c| c.usage()).collect(),
            mem_total: self.sys.total_memory(),
            mem_used: self.sys.used_memory(),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
            load1: load.as_ref().map(|l| l.one as f32).unwrap_or(0.0),
            load5: load.as_ref().map(|l| l.five as f32).unwrap_or(0.0),
            load15: load.as_ref().map(|l| l.fifteen as f32).unwrap_or(0.0),
            uptime_secs: System::uptime().unwrap_or(0),
            net_in_total: in_total,
            net_out_total: out_total,
            net_in_speed: in_speed,
            net_out_speed: out_speed,
            disks,
            gpus,
            processes,
            process_count,
            tcp_conn_count: 0, // M6: Linux 下解析 /proc/net/tcp
            udp_conn_count: 0,
        }
    }

    /// 真实磁盘列表（过滤伪文件系统与容器重复卷）。
    ///
    /// - 排除：devfs/map 等伪挂载、tmpfs/squashfs/overlay 等内存或只读镜像 fs
    /// - macOS：APFS 各卷（/System/Volumes/Data、Preboot、VM…）共享同一容器，
    ///   available 都是容器口径，逐卷展示会出现"每个卷都占 150G"的假象 →
    ///   只保留挂载点 "/"，其 used = 容器真实占用（total - available）
    fn disk_list(&mut self) -> Vec<DiskUsage> {
        const VIRTUAL_FS: &[&str] = &[
            "devfs", "devtmpfs", "tmpfs", "squashfs", "erofs", "overlay", "iso9660",
            "procfs", "autofs", "fusefs", "msdosfs", "vfat", "cd9660", "nullfs",
        ];
        self.disks.refresh(false);
        self.disks
            .iter()
            .filter(|d| {
                let mount = d.mount_point().to_string_lossy();
                let fs = d.file_system().to_string_lossy();
                if d.total_space() == 0 {
                    return false;
                }
                if mount.starts_with("/dev") || mount.starts_with("map ") {
                    return false;
                }
                if VIRTUAL_FS.contains(&fs.as_ref()) {
                    return false;
                }
                // macOS 系统卷/数据卷与 / 同容器，只保留 /
                if cfg!(target_os = "macos") && mount.starts_with("/System/Volumes/") {
                    return false;
                }
                true
            })
            .map(|d| DiskUsage {
                mount: d.mount_point().to_string_lossy().into_owned(),
                fs: d.file_system().to_string_lossy().into_owned(),
                total: d.total_space(),
                used: d.total_space().saturating_sub(d.available_space()),
            })
            .collect()
    }

    /// 低频完整报告。
    pub fn full_report(&mut self) -> SystemInfo {
        self.sys.refresh_memory();
        let disks_total: u64 = self
            .disk_list()
            .iter()
            .map(|d| d.total)
            .max()
            .unwrap_or(0);

        SystemInfo {
            hostname: System::host_name().unwrap_or_else(|_| "unknown".into()),
            os: System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.into()),
            kernel: System::kernel_long_version().unwrap_or_default(),
            arch: System::cpu_arch(),
            cpu_model: self
                .sys
                .cpus()
                .first()
                .map(|c| c.brand().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".into()),
            cpu_cores: System::physical_core_count().unwrap_or(0) as u32,
            mem_total: self.sys.total_memory(),
            swap_total: self.sys.total_swap(),
            disk_total: disks_total,
            virt: detect_virt(),
            boot_time: System::boot_time().unwrap_or(0),
        }
    }
}

fn detect_virt() -> String {
    // 轻量虚拟化探测：只读 /proc 或系统调用，不执行外部命令
    #[cfg(target_os = "linux")]
    {
        if let Ok(v) = std::fs::read_to_string("/proc/xen") {
            let _ = v;
            return "xen".into();
        }
        if let Ok(dmi) = std::fs::read_to_string("/sys/class/dmi/id/product_name") {
            let dmi = dmi.to_lowercase();
            if dmi.contains("kvm") || dmi.contains("qemu") {
                return "kvm".into();
            }
            if dmi.contains("vmware") {
                return "vmware".into();
            }
            if dmi.contains("virtualbox") {
                return "virtualbox".into();
            }
        }
        if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup") {
            if cgroup.contains("lxc") {
                return "lxc".into();
            }
            if cgroup.contains("docker") || cgroup.contains("containerd") {
                return "container".into();
            }
        }
        if std::path::Path::new("/proc/vz").exists() {
            return "openvz".into();
        }
        "none".into()
    }
    #[cfg(not(target_os = "linux"))]
    {
        "unknown".into()
    }
}
