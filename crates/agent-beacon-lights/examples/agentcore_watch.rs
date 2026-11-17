//! 诊断工具：连上 AgentCore 任一端点，原样打印 N 秒内收到的所有上报行。
//!
//! ```bash
//! # BLE(NUS)
//! cargo run -p agent-beacon-lights --example agentcore_watch -- ble:AA:BB:CC:DD:EE:FF 20
//! # 串口 / USB CDC
//! cargo run -p agent-beacon-lights --example agentcore_watch -- /dev/ttyACM0 20
//! # 免 root SPP
//! cargo run -p agent-beacon-lights --example agentcore_watch -- spp:AA:BB:CC:DD:EE:FF 20
//! ```
//!
//! 用来核对固件实际上报（`CHG:` 心跳、`STATUS:`、`SETTINGS:` …），排查“界面显示与设备
//! 实际状态不一致”这类问题。设备插着 USB 时 BLE 不广播，用 BLE 前先拔线或直接给地址。

use std::thread::sleep;
use std::time::{Duration, Instant};

use agent_beacon_lights::transport::{AgentCoreTransport, Transport};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(target) = args.next() else {
        eprintln!("usage: agentcore_watch <port|ble:ADDR|spp:ADDR> [seconds]");
        std::process::exit(2);
    };
    let seconds: u64 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);

    let opened = if let Some(address) = target.strip_prefix("ble:") {
        agent_beacon_lights::ble::BleTransport::open(address)
            .map(|t| Box::new(t) as Box<dyn Transport>)
    } else if let Some(address) = target.strip_prefix("spp:") {
        AgentCoreTransport::open_spp(address).map(|t| Box::new(t) as Box<dyn Transport>)
    } else {
        AgentCoreTransport::open(&target).map(|t| Box::new(t) as Box<dyn Transport>)
    };

    let mut transport = match opened {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("connect failed: {error}");
            std::process::exit(1);
        }
    };
    println!("connected: {}", transport.describe());

    if let Err(error) = transport.write(b"STATUS\n") {
        eprintln!("write STATUS failed: {error}");
        std::process::exit(1);
    }

    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        for line in transport.drain_lines() {
            let elapsed = started.elapsed().as_secs_f32();
            println!("{elapsed:6.1}s < {line}");
        }
        sleep(Duration::from_millis(200));
    }
}
