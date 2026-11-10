//! 诊断工具：连上 BLE（NUS）链路，读一次 STATUS，再回到 IDLE。
//!
//! ```bash
//! cargo run -p agent-beacon-lights --example ble_status -- AA:BB:CC:DD:EE:FF
//! ```
//!
//! 设备插着 USB 时不广播，用之前先拔线。

use std::thread::sleep;
use std::time::Duration;

use agent_beacon_lights::transport::Transport;

fn main() {
    let Some(selector) = std::env::args().nth(1) else {
        eprintln!("usage: ble_status <address|name:AgentCore-Light-BLE>");
        std::process::exit(2);
    };
    let mut transport = match agent_beacon_lights::ble::BleTransport::open(&selector) {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("connect failed: {error}");
            std::process::exit(1);
        }
    };
    println!("connected: {}", transport.describe());

    for command in ["STATUS", "GET_SETTINGS"] {
        if let Err(error) = transport.write(format!("{command}\n").as_bytes()) {
            eprintln!("write {command} failed: {error}");
            std::process::exit(1);
        }
        sleep(Duration::from_millis(400));
        for line in transport.drain_lines() {
            println!("< {line}");
        }
    }

    sleep(Duration::from_millis(600));
    for line in transport.drain_lines() {
        println!("< {line}");
    }
}
