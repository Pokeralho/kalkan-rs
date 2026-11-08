//! RAM física, swap (arquivo de paginação), ocupação e throughput de disco.

use super::pdh::{Counter, PdhQuery};
use super::Snapshot;
use sysinfo::{Disks, MemoryRefreshKind, RefreshKind, System};

pub struct MemoryCollector {
    sys: System,
    disks: Disks,
    mount: String,
    read: Option<Counter>,
    write: Option<Counter>,
    tick: u32,
}

impl MemoryCollector {
    pub fn new(pdh: Option<&PdhQuery>, mount: String) -> Self {
        Self {
            sys: System::new_with_specifics(RefreshKind::new().with_memory(MemoryRefreshKind::everything())),
            disks: Disks::new_with_refreshed_list(),
            mount,
            read: pdh.and_then(|p| p.add(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec")),
            write: pdh.and_then(|p| p.add(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec")),
            tick: 0,
        }
    }

    pub fn update(&mut self, s: &mut Snapshot, pdh: Option<&PdhQuery>) {
        const MB: f64 = 1024.0 * 1024.0;
        const GB: f64 = MB * 1024.0;
        self.sys.refresh_memory();
        s.ram_total = Some(self.sys.total_memory() as f64 / MB);
        s.ram_used = Some(self.sys.used_memory() as f64 / MB);
        s.swap_total = Some(self.sys.total_swap() as f64 / MB);
        s.swap_used = Some(self.sys.used_swap() as f64 / MB);

        // Ocupação do disco muda devagar: atualiza a cada 10 s.
        if self.tick % 10 == 0 {
            self.disks.refresh();
            let want = self.mount.trim_end_matches('\\').to_ascii_uppercase();
            if let Some(d) = self.disks.iter().find(|d| {
                d.mount_point().to_string_lossy().trim_end_matches('\\').to_ascii_uppercase() == want
            }) {
                let total = d.total_space() as f64;
                s.disk_total_gb = Some(total / GB);
                s.disk_used_gb = Some((total - d.available_space() as f64) / GB);
            }
        }
        self.tick = self.tick.wrapping_add(1);

        if let Some(p) = pdh {
            s.disk_read = self.read.and_then(|c| p.value(c));
            s.disk_write = self.write.and_then(|c| p.value(c));
        }
    }
}
