//! 端到端：启动真实二进制，用 CLI 上报，验证聚合、会话与 stdin 关闭退出。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_beacon_core::Config;
use serde_json::Value;

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn temp_config(port: u16) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("agent-beacon-e2e-{unique}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.json");
    let mut config = Config::default();
    config.server.port = port;
    std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
    path
}

fn wait_until<F: FnMut() -> bool>(timeout: Duration, mut condition: F) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn cli_report_updates_state_and_shuts_down_on_stdin_close() {
    let port = free_port();
    let config = temp_config(port);
    let base = format!("http://127.0.0.1:{port}");
    let bin = env!("CARGO_BIN_EXE_agent-beacon");

    let mut child = Command::new(bin)
        .arg("serve")
        .arg("--config")
        .arg(&config)
        .arg("--port")
        .arg(port.to_string())
        .arg("--supervised")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 agent-beacon");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .unwrap();

    assert!(
        wait_until(Duration::from_secs(10), || {
            client
                .get(format!("{base}/api/v1/health"))
                .send()
                .map(|response| response.status().is_success())
                .unwrap_or(false)
        }),
        "服务未在 10 秒内健康"
    );

    let report = Command::new(bin)
        .arg("report")
        .arg("--url")
        .arg(&base)
        .arg("--agent")
        .arg("claude")
        .arg("--state")
        .arg("waiting")
        .arg("--message")
        .arg("等待确认")
        .output()
        .expect("执行 report");
    assert!(
        report.status.success(),
        "report 失败：{}",
        String::from_utf8_lossy(&report.stderr)
    );

    assert!(
        wait_until(Duration::from_secs(5), || {
            let Ok(response) = client.get(format!("{base}/api/v1/state")).send() else {
                return false;
            };
            let Ok(value) = response.json::<Value>() else {
                return false;
            };
            value["aggregate"] == "waiting"
                && value["sessions"].as_array().is_some_and(|sessions| {
                    sessions.iter().any(|session| {
                        session["agent"] == "claude" && session["state"] == "waiting"
                    })
                })
        }),
        "CLI 上报没有进入 waiting 状态"
    );

    let _ = child.stdin.take();
    assert!(
        wait_until(Duration::from_secs(5), || {
            child
                .try_wait()
                .map(|status| status.is_some())
                .unwrap_or(false)
        }),
        "stdin 关闭后服务没有退出"
    );
    let status = child.wait().unwrap();
    assert!(status.success(), "服务退出码异常：{status}");

    let _ = std::fs::remove_dir_all(config.parent().unwrap());
}
