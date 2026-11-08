//! Velocidade de download/upload somando todas as interfaces físicas.

use super::Snapshot;
use std::time::Instant;
use sysinfo::Networks;

pub struct NetworkCollector {
    nets: Networks,
    last: Instant,
    tick: u32,
}

impl NetworkCollector {
    pub fn new() -> Self {
        Self { nets: Networks::new_with_refreshed_list(), last: Instant::now(), tick: 0 }
    }

    pub fn update(&mut self, s: &mut Snapshot) {
        // Re-enumera interfaces de vez em quando (VPN, Wi-Fi conectando etc).
        if self.tick % 30 == 29 {
            self.nets.refresh_list();
        } else {
            self.nets.refresh();
        }
        self.tick = self.tick.wrapping_add(1);

        let dt = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();

        let (mut rx, mut tx) = (0u64, 0u64);
        for (name, data) in self.nets.iter() {
            let n = name.to_ascii_lowercase();
            if n.contains("loopback") || n.contains("vethernet") || n.contains("hyper-v") {
                continue;
            }
            rx += data.received();
            tx += data.transmitted();
        }
        s.net_down = Some(rx as f64 / dt);
        s.net_up = Some(tx as f64 / dt);
    }
}
