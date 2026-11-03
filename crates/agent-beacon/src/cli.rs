//! 命令行入口：serve / report / status / devices。

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use agent_beacon_core::{AgentState, Config};
use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

use crate::api;
use crate::config_store::ConfigStore;
use crate::integrations::{self, Outcome, Target};
use crate::state::BeaconState;
use crate::worker;

#[derive(Debug, Parser)]
#[command(name = "agent-beacon", version, about = "同步 agent 状态到 USB 状态灯")]
pub struct Cli {
    /// 服务地址（report/status/devices 使用）。
    #[arg(long, global = true, default_value = "http://127.0.0.1:6980")]
    pub url: String,

    /// 日志级别（RUST_LOG 优先）。
    #[arg(long, global = true, default_value = "info")]
    pub log_level: String,

    /// 集成插件目录（integrate 命令使用；单目标时可用）。
    #[arg(long, global = true)]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 启动常驻服务（默认子命令）。
    Serve(ServeArgs),
    /// 上报一条 agent 状态事件。
    Report(ReportArgs),
    /// 输出服务健康状况与聚合状态。
    Status,
    /// 输出设备列表。
    Devices,
    /// 安装/升级 agent 集成插件。
    Integrate(IntegrateArgs),
}

#[derive(Debug, Default, Args)]
pub struct ServeArgs {
    /// 覆盖配置中的监听端口。
    #[arg(long)]
    pub port: Option<u16>,
    /// 覆盖配置中的监听地址。
    #[arg(long)]
    pub host: Option<String>,
    /// 由桌面壳托管：stdin 关闭时退出。
    #[arg(long)]
    pub supervised: bool,
    /// 指定配置文件路径。
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct IntegrateArgs {
    #[command(subcommand)]
    pub action: IntegrateAction,
}

#[derive(Debug, Subcommand)]
pub enum IntegrateAction {
    /// 查看支持项与安装状态。
    List,
    /// 安装插件；不指定 target 时安装全部。
    Install {
        /// pi | opencode | dsh | all
        target: Option<String>,
        /// 覆盖非 agent-beacon 管理的同名文件。
        #[arg(long)]
        force: bool,
    },
    /// 升级已安装插件；不指定 target 时升级全部。
    Upgrade {
        /// pi | opencode | dsh | all
        target: Option<String>,
    },
    /// 卸载插件。
    Uninstall { target: String },
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    #[arg(long)]
    pub agent: String,
    #[arg(long)]
    pub state: String,
    #[arg(long)]
    pub session: Option<String>,
    #[arg(long)]
    pub message: Option<String>,
    #[arg(long)]
    pub ttl: Option<u64>,
}

pub async fn run(mut cli: Cli) -> i32 {
    init_tracing(&cli.log_level);
    let command = cli.command.take();
    match command {
        Some(Command::Serve(args)) => serve(args).await,
        Some(Command::Report(args)) => report(&cli, args).await,
        Some(Command::Status) => status(&cli).await,
        Some(Command::Devices) => devices(&cli).await,
        Some(Command::Integrate(args)) => integrate(args, cli.dir.as_deref()),
        None => serve(ServeArgs::default()).await,
    }
}

fn init_tracing(level: &str) {
    let filter = EnvFilter::try_new(level)
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

async fn serve(args: ServeArgs) -> i32 {
    let store = match args.config {
        Some(path) => ConfigStore::new(path),
        None => match ConfigStore::default_path() {
            Ok(path) => ConfigStore::new(path),
            Err(error) => {
                eprintln!("{error}");
                return 1;
            }
        },
    };

    let mut config: Config = match store.load_or_create() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    if let Some(port) = args.port {
        config.server.port = port;
    }
    if let Some(host) = args.host {
        config.server.host = host;
    }
    if let Err(error) = config.validate() {
        eprintln!("配置非法：{error}");
        return 1;
    }

    let host = config.server.host.clone();
    let port = config.server.port;
    let probe_host = if host == "0.0.0.0" || host == "::" {
        "127.0.0.1"
    } else {
        host.as_str()
    };

    if health_online(probe_host, port).await {
        println!("agent-beacon: listening addr={host}:{port}");
        flush_stdout();
        eprintln!("已有 agent-beacon 实例在运行，复用现有实例并退出");
        return 0;
    }

    let listener = match tokio::net::TcpListener::bind((host.as_str(), port)).await {
        Ok(listener) => listener,
        Err(error) => {
            let kind = bind_kind(&error);
            println!("agent-beacon: bind-failed addr={host}:{port} kind={kind}");
            flush_stdout();
            eprintln!("无法监听 {host}:{port}（{error}）");
            return 1;
        }
    };
    let actual_port = listener
        .local_addr()
        .map(|address| address.port())
        .unwrap_or(port);

    println!("agent-beacon: listening addr={host}:{actual_port}");
    flush_stdout();
    tracing::info!(address = %format!("{host}:{actual_port}"), "agent-beacon 已启动");

    // 已安装的受管插件随二进制版本自动升级；不主动安装新插件。
    for (target, result) in integrations::upgrade_installed() {
        match result {
            Ok(Outcome::Upgraded) => {
                tracing::info!(agent = target.id(), "已自动升级 agent 集成插件");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(agent = target.id(), %error, "自动升级 agent 集成插件失败");
            }
        }
    }

    let state = BeaconState::new(config.clone(), store, actual_port);
    let app = api::router(state.clone());
    worker::spawn_worker(state.clone());

    let ticker = {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                state.tick();
            }
        })
    };

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    if args.supervised {
        spawn_stdin_watchdog(shutdown_tx.clone());
    }
    spawn_signal_watchdog(shutdown_tx.clone());

    let shutdown_state = state.clone();
    let shutdown = async move {
        while !*shutdown_rx.borrow() {
            if shutdown_rx.changed().await.is_err() {
                break;
            }
        }
        shutdown_state.begin_shutdown();
    };

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;

    ticker.abort();
    if let Err(error) = result {
        eprintln!("服务异常退出：{error}");
        return 1;
    }

    if config.shutdown.turn_off_lights {
        state.force_off();
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    0
}

fn flush_stdout() {
    let _ = std::io::stdout().flush();
}

fn bind_kind(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => "address-in-use",
        std::io::ErrorKind::PermissionDenied => "permission-denied",
        std::io::ErrorKind::AddrNotAvailable => "address-not-available",
        _ => "bind-error",
    }
}

fn spawn_stdin_watchdog(tx: watch::Sender<bool>) {
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut buffer = [0u8; 256];
        loop {
            match std::io::stdin().read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        let _ = tx.send(true);
    });
}

fn spawn_signal_watchdog(tx: watch::Sender<bool>) {
    tokio::spawn(async move {
        wait_for_signal().await;
        let _ = tx.send(true);
    });
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health_online(host: &str, port: u16) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(400))
        .build()
    else {
        return false;
    };
    client
        .get(format!("http://{host}:{port}/api/v1/health"))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

fn parse_state(value: &str) -> Result<AgentState, String> {
    match value {
        "idle" => Ok(AgentState::Idle),
        "working" => Ok(AgentState::Working),
        "waiting" => Ok(AgentState::Waiting),
        "error" => Ok(AgentState::Error),
        other => Err(format!(
            "未知状态 {other}，合法值为 idle|working|waiting|error"
        )),
    }
}

async fn report(cli: &Cli, args: ReportArgs) -> i32 {
    let state = match parse_state(&args.state) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(900))
        .build()
    else {
        eprintln!("无法创建 HTTP 客户端");
        return 2;
    };

    let mut body = json!({
        "agent": args.agent,
        "state": state,
    });
    if let Some(session) = args.session {
        body["session"] = json!(session);
    }
    if let Some(message) = args.message {
        body["message"] = json!(message);
    }
    if let Some(ttl) = args.ttl {
        body["ttl_secs"] = json!(ttl);
    }

    let url = format!("{}/api/v1/events", cli.url.trim_end_matches('/'));
    match client.post(url).json(&body).send().await {
        Ok(response) if response.status().is_success() => 0,
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("上报失败（HTTP {status}）：{text}");
            2
        }
        Err(error) => {
            eprintln!("无法连接 agent-beacon（{}）：{error}", cli.url);
            2
        }
    }
}

async fn status(cli: &Cli) -> i32 {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        eprintln!("无法创建 HTTP 客户端");
        return 2;
    };
    let url = format!("{}/api/v1/state", cli.url.trim_end_matches('/'));
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            eprintln!("读取状态失败（HTTP {}）", response.status());
            return 2;
        }
        Err(error) => {
            eprintln!("无法连接 agent-beacon（{}）：{error}", cli.url);
            return 2;
        }
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("解析状态失败：{error}");
            return 2;
        }
    };
    print_status(&value, &cli.url);
    0
}

fn print_status(value: &Value, url: &str) {
    let service = &value["service"];
    println!(
        "服务: {} (v{}, pid {}, 已运行 {} 秒)",
        url,
        service["version"].as_str().unwrap_or("?"),
        service["pid"].as_u64().unwrap_or(0),
        service["uptime_secs"].as_u64().unwrap_or(0)
    );
    println!(
        "聚合状态: {}",
        value["aggregate"].as_str().unwrap_or("none")
    );
    println!(
        "活动会话: {}",
        value["sessions"].as_array().map(Vec::len).unwrap_or(0)
    );
    let devices = value["devices"].as_array().cloned().unwrap_or_default();
    let connected = devices
        .iter()
        .filter(|device| device["connected"].as_bool().unwrap_or(false))
        .count();
    println!("设备: {}/{} 已连接", connected, devices.len());
}

async fn devices(cli: &Cli) -> i32 {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        eprintln!("无法创建 HTTP 客户端");
        return 2;
    };
    let url = format!("{}/api/v1/devices", cli.url.trim_end_matches('/'));
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            eprintln!("读取设备失败（HTTP {}）", response.status());
            return 2;
        }
        Err(error) => {
            eprintln!("无法连接 agent-beacon（{}）：{error}", cli.url);
            return 2;
        }
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("解析设备列表失败：{error}");
            return 2;
        }
    };
    let devices = value.as_array().cloned().unwrap_or_default();
    if devices.is_empty() {
        println!("没有发现受支持的设备");
        return 0;
    }
    println!("{:<28} {:<14} {:<10} {}", "ID", "PORT", "PROFILE", "状态");
    for device in devices {
        println!(
            "{:<28} {:<14} {:<10} {}",
            device["id"].as_str().unwrap_or("?"),
            device["port"].as_str().unwrap_or("?"),
            device["profile"].as_str().unwrap_or("?"),
            if device["connected"].as_bool().unwrap_or(false) {
                format!("已连接 · {}", device["effect"].as_str().unwrap_or("off"))
            } else {
                "未连接".to_string()
            }
        );
    }
    0
}

fn parse_targets(raw: Option<&str>) -> Result<Vec<Target>, String> {
    let value = raw.unwrap_or("all").trim();
    match value {
        "" | "all" => Ok(Target::ALL
            .iter()
            .copied()
            .filter(|target| *target != Target::Dsh || crate::dsh::available())
            .collect()),
        "pi" => Ok(vec![Target::Pi]),
        "opencode" => Ok(vec![Target::OpenCode]),
        "dsh" => Ok(vec![Target::Dsh]),
        other => Err(format!("未知集成 {other}；可选：pi、opencode、dsh、all")),
    }
}

fn integrate(args: IntegrateArgs, dir: Option<&std::path::Path>) -> i32 {
    match args.action {
        IntegrateAction::List => {
            println!(
                "{:<9} {:<8} {:<8} {}",
                "AGENT", "INSTALLED", "BUNDLED", "STATE"
            );
            let mut code = 0;
            for target in Target::ALL {
                match integrations::status(target, dir) {
                    Ok(status) => {
                        println!(
                            "{:<9} {:<8} {:<8} {}",
                            status.target.id(),
                            status.installed_version.as_deref().unwrap_or("-"),
                            status.embedded_version,
                            status.state.label()
                        );
                        println!("          {}", status.dir.display());
                    }
                    Err(error) => {
                        eprintln!("{}: {error}", target.id());
                        code = 1;
                    }
                }
            }
            code
        }
        IntegrateAction::Install { target, force } => {
            let targets = match parse_targets(target.as_deref()) {
                Ok(targets) => targets,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            if dir.is_some() && targets.len() > 1 {
                eprintln!("--dir 只能与单个 target 一起使用");
                return 2;
            }
            let mut code = 0;
            for target in targets {
                match integrations::install(target, dir, force) {
                    Ok((outcome, path)) => {
                        println!(
                            "{}: {} -> {}",
                            target.label(),
                            outcome.label(),
                            path.display()
                        );
                    }
                    Err(error) => {
                        eprintln!("{}: {error}", target.label());
                        code = 1;
                    }
                }
            }
            code
        }
        IntegrateAction::Upgrade { target } => {
            let targets = match parse_targets(target.as_deref()) {
                Ok(targets) => targets,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            if dir.is_some() && targets.len() > 1 {
                eprintln!("--dir 只能与单个 target 一起使用");
                return 2;
            }
            let mut code = 0;
            for target in targets {
                match integrations::upgrade(target, dir) {
                    Ok((outcome, path)) => {
                        println!(
                            "{}: {} -> {}",
                            target.label(),
                            outcome.label(),
                            path.display()
                        );
                    }
                    Err(error) => {
                        eprintln!("{}: {error}", target.label());
                        code = 1;
                    }
                }
            }
            code
        }
        IntegrateAction::Uninstall { target } => {
            let target = match Target::parse(target.trim()) {
                Some(target) => target,
                None => {
                    eprintln!("未知集成 {target}；可选：pi、opencode、dsh");
                    return 2;
                }
            };
            match integrations::uninstall(target, dir) {
                Ok(path) => {
                    println!("{}: 已卸载 {}", target.label(), path.display());
                    0
                }
                Err(error) => {
                    eprintln!("{}: {error}", target.label());
                    1
                }
            }
        }
    }
}
