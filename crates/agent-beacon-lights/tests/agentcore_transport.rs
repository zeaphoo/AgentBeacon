//! 用 socat 造一对虚拟串口，验证 `AgentCoreTransport` 的握手、分行与上报读取。
//!
//! 没有 socat 的环境（多数 CI）会直接跳过，不会让测试失败。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use agent_beacon_lights::agentcore::{parse_report, Report};
use agent_beacon_lights::transport::Transport;

/// 起一对虚拟串口，返回 (设备侧路径, 程序侧路径, 子进程)。
fn socat_pair(tag: &str) -> Option<(String, String, Child)> {
    let base = std::env::temp_dir().join(format!("agent-beacon-ac-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&base).ok()?;
    let device = base.join("device");
    let host = base.join("host");
    let child = Command::new("socat")
        .arg("-d")
        .arg("-d")
        .arg("pty,raw,echo=0")
        .arg("pty,raw,echo=0")
        .arg(format!("link={}", device.display()))
        .arg(format!("link={}", host.display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if device.exists() && host.exists() {
            return Some((
                device.to_string_lossy().to_string(),
                host.to_string_lossy().to_string(),
                child,
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// 假设备：`HELLO` 回 `HELLO:OK`，`STATUS` 回状态行，之后持续推 `CHG:` 心跳。
fn fake_device(path: String) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let Ok(stream) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
        else {
            return;
        };
        let mut writer = match stream.try_clone() {
            Ok(handle) => handle,
            Err(_) => return,
        };
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let Ok(line) = line else { return };
            let command = line.trim();
            let reply: Option<&[u8]> = match command {
                "HELLO" => Some(b"HELLO:OK\r\n"),
                "STATUS" => Some(
                    b"STATUS:IDLE,FW:v0.5.13-jl-ota,BRIGHTNESS_LEVEL:30,BUZZER:ON,BUZZER_VOLUME:10,BLE_STATE:5,BLE_ADV:0,BLE_CONN:0\r\n",
                ),
                "BUSY" => Some(b"STATE:BUSY\r\n"),
                _ => None,
            };
            if let Some(reply) = reply {
                if writer.write_all(reply).and_then(|()| writer.flush()).is_err() {
                    return;
                }
            }
            // 心跳：验证读线程会把 `CHG:` 收进队列。
            let _ = writer
                .write_all(b"CHG:1,VBAT:311,PER:0,CUR:100\r\n")
                .and_then(|()| writer.flush());
        }
    })
}

#[test]
fn handshakes_and_drains_reports_over_a_virtual_port() {
    let Some((device, host, mut socat)) = socat_pair("transport") else {
        eprintln!("socat 不可用，跳过虚拟串口测试");
        return;
    };
    let fake = fake_device(device);

    // 打开会先等 1.5 s（CDC settle）再做 HELLO 握手。
    let mut transport = match agent_beacon_lights::transport::AgentCoreTransport::open(&host) {
        Ok(transport) => transport,
        Err(error) => panic!("虚拟端口应当握手成功：{error}"),
    };

    // 设备每收到一行就回一行 + 一次 CHG，读线程应当把它们收进来。
    transport.write(b"BUSY\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut lines = Vec::new();
    while Instant::now() < deadline {
        lines.extend(transport.drain_lines());
        if lines
            .iter()
            .any(|line| matches!(parse_report(line), Report::Battery(_)))
            && lines
                .iter()
                .any(|line| matches!(parse_report(line), Report::State(_)))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        lines.iter().any(|line| line == "STATE:BUSY"),
        "应当读到状态回显：{lines:?}"
    );
    let battery = lines
        .iter()
        .find_map(|line| match parse_report(line) {
            Report::Battery(battery) => Some(battery),
            _ => None,
        })
        .expect("应当读到电量心跳");
    assert!(battery.charging);
    assert_eq!(battery.millivolts, Some(311));

    // 队列取空后不应重复给出同一批行。
    assert!(transport.drain_lines().is_empty());

    drop(transport);
    let _ = socat.kill();
    let _ = socat.wait();
    let _ = fake.join();
}

#[test]
fn rejects_a_port_that_never_answers_hello() {
    let Some((device, host, mut socat)) = socat_pair("noanswer") else {
        eprintln!("socat 不可用，跳过虚拟串口测试");
        return;
    };
    // 只占用设备侧端口，不回任何内容。
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&device)
        .ok();

    let error = match agent_beacon_lights::transport::AgentCoreTransport::open(&host) {
        Ok(_) => panic!("没有 HELLO 应答的端口不应通过握手"),
        Err(error) => error,
    };
    assert!(
        error.contains("HELLO"),
        "握手失败应说明原因，实际：{error}"
    );

    drop(holder);
    let _ = socat.kill();
    let _ = socat.wait();
}