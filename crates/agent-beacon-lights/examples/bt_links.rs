//! 诊断工具：列出 BlueZ 里**已知**的 AgentCore-Light 链路（不扫描、不依赖广播）。
//!
//! ```bash
//! cargo run -p agent-beacon-lights --example bt_links
//! ```
//!
//! 与 `ble_scan` 的区别：这里不触发 BLE 发现，因此
//! * 设备已经连着/刚连过（不广播）时也能看到它；
//! * 不会打断已有的 LE 连接；
//! * 同时能看到经典蓝牙那条（`spp:`），它是 SPP 兜底连接的目标。

fn main() {
    match agent_beacon_lights::ble::known_links() {
        Ok(links) if links.is_empty() => {
            println!("no known AgentCore-Light; pair it first, or run ble_scan to look for advertisements");
        }
        Ok(links) => {
            for link in links {
                println!(
                    "{:<6} {:<18} {}",
                    match link {
                        agent_beacon_lights::ble::KnownLink::Ble(_) => "ble",
                        agent_beacon_lights::ble::KnownLink::Spp { .. } => "spp",
                    },
                    link.address(),
                    link.name().unwrap_or("-")
                );
                println!("       port = {}", link.port());
            }
        }
        Err(error) => {
            eprintln!("failed: {error}");
            std::process::exit(1);
        }
    }
}
