//! 诊断工具：用免 root 的 SPP（RFCOMM socket）直连 AgentCore-Light。
//!
//! ```bash
//! cargo run -p agent-beacon-lights --example spp_probe -- AF:C3:A4:E1:D8:95
//! cargo run -p agent-beacon-lights --example spp_probe -- AF:C3:A4:E1:D8:95:2
//! ```
//!
//! 它做三件事：开 RFCOMM 通道 → `HELLO` 握手 → 发一次 `STATUS` 并打印设备上报。
//! 不依赖 `/dev/rfcomm*` 节点，也不需要 root（只要设备已配对）。

use std::time::{Duration, Instant};

use agent_beacon_lights::transport::probe_agentcore_spp;

fn main() {
    let Some(target) = std::env::args().nth(1) else {
        eprintln!("usage: spp_probe <AA:BB:CC:DD:EE:FF[:channel]>");
        std::process::exit(2);
    };

    println!("connecting to SPP target {target} ...");
    let started = Instant::now();
    match probe_agentcore_spp(&target) {
        Ok(identity) => {
            println!("handshake OK in {:?}", started.elapsed());
            println!("  firmware        : {:?}", identity.firmware);
            println!("  state           : {:?}", identity.state);
            println!("  brightness_level: {:?}", identity.brightness_level);
        }
        Err(error) => {
            eprintln!("failed after {:?}: {error}", started.elapsed());
            std::process::exit(1);
        }
    }

    // 设备一次只接受一条 SPP 通道，断开后要给它一点时间释放。
    std::thread::sleep(Duration::from_millis(1500));
    match agent_beacon_lights::transport::probe_agentcore_spp(&target) {
        Ok(identity) => println!("reconnect OK (state {:?})", identity.state),
        Err(error) => {
            eprintln!("reconnect failed: {error}");
            std::process::exit(1);
        }
    }
}
