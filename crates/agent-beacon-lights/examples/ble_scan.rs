//! 诊断工具：扫一遍 BLE 广播，看看能不能找到 AgentCore-Light。
//!
//! ```bash
//! cargo run -p agent-beacon-lights --example ble_scan -- 8
//! ```
//!
//! 设备插着 USB 时不广播，用之前先拔线。

use std::time::Duration;

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(6);
    println!("scanning BLE for {secs}s ...");
    match agent_beacon_lights::ble::scan(Duration::from_secs(secs)) {
        Ok(devices) if devices.is_empty() => println!("no AgentCore-Light found"),
        Ok(devices) => {
            for device in devices {
                println!(
                    "{}  {}",
                    device.address,
                    device.name.unwrap_or_else(|| "-".to_string())
                );
            }
        }
        Err(error) => {
            eprintln!("scan failed: {error}");
            std::process::exit(1);
        }
    }
}
