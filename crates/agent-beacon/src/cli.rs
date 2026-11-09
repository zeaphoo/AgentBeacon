//! 命令行入口：serve / report / status / devices。

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use agent_beacon_core::{now_ms, AgentState, Config};
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
#[command(
    name = "agent-beacon",
    version,
    about = "Sync agent status to USB status lights"
)]
pub struct Cli {
    /// Service URL (used by report/status/devices).
    #[arg(long, global = true, default_value = "http://127.0.0.1:6980")]
    pub url: String,

    /// Log level (RUST_LOG takes precedence).
    #[arg(long, global = true, default_value = "info")]
    pub log_level: String,

    /// Integration plugin directory (for integrate; only with a single target).
    #[arg(long, global = true)]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the long-running service (default subcommand).
    Serve(ServeArgs),
    /// Report an agent status event.
    Report(ReportArgs),
    /// Print service health and aggregate state.
    Status,
    /// List devices.
    Devices,
    /// Show recent hook and state-transition diagnostics.
    Hooks(HooksArgs),
    /// Report a status event from an agent hook (reads the hook payload).
    Hook(HookArgs),
    /// Install or upgrade agent integration plugins.
    Integrate(IntegrateArgs),
}

#[derive(Debug, Default, Args)]
pub struct ServeArgs {
    /// Override the configured listen port.
    #[arg(long)]
    pub port: Option<u16>,
    /// Override the configured listen host.
    #[arg(long)]
    pub host: Option<String>,
    /// Supervised by the desktop shell: exit when stdin closes.
    #[arg(long)]
    pub supervised: bool,
    /// Path to the config file.
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct HooksArgs {
    /// Maximum number of entries to show (1-1000).
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    /// Print the raw JSON response instead of a table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct HookArgs {
    /// Agent name: claude-code | codex | gemini | qwen | iflow | qoder.
    #[arg(long)]
    pub agent: String,
    /// State to report: idle | working | waiting | error | auto (derive from the event).
    #[arg(long, default_value = "auto")]
    pub state: String,
    /// Lifecycle event name, if it is not part of the payload.
    #[arg(long)]
    pub event: Option<String>,
    /// Status message override (defaults to the tool name or event).
    #[arg(long)]
    pub message: Option<String>,
    /// Session TTL in seconds.
    #[arg(long)]
    pub ttl: Option<u64>,
    /// Hook payload passed as an argument (Codex notify style).
    #[arg(long)]
    pub payload: Option<String>,
    /// Trailing argument that may carry the payload.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub extra: Vec<String>,
}

#[derive(Debug, Args)]
pub struct IntegrateArgs {
    #[command(subcommand)]
    pub action: IntegrateAction,
}

#[derive(Debug, Subcommand)]
pub enum IntegrateAction {
    /// Show supported integrations and installation state.
    List,
    /// Install plugins; installs all detected targets when target is omitted.
    Install {
        /// pi | opencode | dsh | claude-code | codex | gemini | qwen | iflow | qoder | all
        target: Option<String>,
        /// Overwrite files not managed by agent-beacon.
        #[arg(long)]
        force: bool,
    },
    /// Upgrade installed plugins; upgrades all targets when target is omitted.
    Upgrade {
        /// pi | opencode | dsh | claude-code | codex | gemini | qwen | iflow | qoder | all
        target: Option<String>,
    },
    /// Uninstall a plugin.
    Uninstall {
        /// pi | opencode | dsh | claude-code | codex | gemini | qwen | iflow | qoder
        target: String,
    },
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Agent name.
    #[arg(long)]
    pub agent: String,
    /// Agent state: idle | working | waiting | error.
    #[arg(long)]
    pub state: String,
    /// Session identifier (optional).
    #[arg(long)]
    pub session: Option<String>,
    /// Status message (optional).
    #[arg(long)]
    pub message: Option<String>,
    /// Session TTL in seconds (optional).
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
        Some(Command::Hooks(args)) => hooks(&cli, args).await,
        Some(Command::Hook(args)) => hook(&cli, args).await,
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
        eprintln!("invalid configuration: {error}");
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
        eprintln!("an agent-beacon instance is already running; reusing it and exiting");
        return 0;
    }

    let listener = match tokio::net::TcpListener::bind((host.as_str(), port)).await {
        Ok(listener) => listener,
        Err(error) => {
            let kind = bind_kind(&error);
            println!("agent-beacon: bind-failed addr={host}:{port} kind={kind}");
            flush_stdout();
            eprintln!("failed to listen on {host}:{port}: {error}");
            return 1;
        }
    };
    let actual_port = listener
        .local_addr()
        .map(|address| address.port())
        .unwrap_or(port);

    println!("agent-beacon: listening addr={host}:{actual_port}");
    flush_stdout();
    tracing::info!(address = %format!("{host}:{actual_port}"), "agent-beacon started");

    // 开启自动安装后，把新检测到的 agent 接进来；随后统一升级已装版本。
    if config.integrations.auto_install {
        for (target, result) in integrations::install_detected() {
            match result {
                Ok(Outcome::Installed) => {
                    tracing::info!(agent = target.id(), "auto-installed agent integration");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(agent = target.id(), %error, "failed to auto-install agent integration");
                }
            }
        }
    }

    // 已安装的受管插件随二进制版本自动升级。
    for (target, result) in integrations::upgrade_installed() {
        match result {
            Ok(Outcome::Upgraded) => {
                tracing::info!(agent = target.id(), "auto-upgraded agent integration");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(agent = target.id(), %error, "failed to auto-upgrade agent integration");
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
        eprintln!("service exited unexpectedly: {error}");
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
            "unknown state {other}; expected idle|working|waiting|error"
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
    post_event(&cli.url, &body).await
}

/// `agent-beacon hook`：供各家 agent 的 hook 调用。
///
/// 状态不可推导时静默返回 0，避免因灯效问题阻塞 agent 自身。
async fn hook(cli: &Cli, args: HookArgs) -> i32 {
    let explicit_state = args.state.trim();
    let state = if explicit_state.is_empty() || explicit_state == "auto" {
        Some("auto".to_string())
    } else {
        match parse_state(explicit_state) {
            Ok(state) => Some(state.as_str().to_string()),
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
    };

    let extra = args.extra.join(" ");
    let payload = args
        .payload
        .or_else(|| (!extra.trim().is_empty()).then_some(extra));
    let request = crate::hookcmd::HookRequest {
        agent: args.agent,
        state,
        event: args.event,
        message: args.message,
        ttl: args.ttl,
        payload,
    };
    let Some(body) = crate::hookcmd::build_body(&request) else {
        return 0;
    };
    // 无论上报是否成功都退出 0：Claude Code 等 agent 会把非零退出码当成
    // 阻断当前操作，灯效不该影响 agent 正常工作。
    let _ = post_event(&cli.url, &body).await;
    0
}

async fn post_event(url: &str, body: &Value) -> i32 {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(900))
        .build()
    else {
        eprintln!("failed to create HTTP client");
        return 2;
    };
    let url = format!("{}/api/v1/events", url.trim_end_matches('/'));
    match client.post(&url).json(body).send().await {
        Ok(response) if response.status().is_success() => 0,
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("report failed (HTTP {status}): {text}");
            2
        }
        Err(error) => {
            eprintln!("cannot connect to agent-beacon ({url}): {error}");
            2
        }
    }
}

async fn status(cli: &Cli) -> i32 {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        eprintln!("failed to create HTTP client");
        return 2;
    };
    let url = format!("{}/api/v1/state", cli.url.trim_end_matches('/'));
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            eprintln!("failed to read status (HTTP {})", response.status());
            return 2;
        }
        Err(error) => {
            eprintln!("cannot connect to agent-beacon ({}): {error}", cli.url);
            return 2;
        }
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("failed to parse status: {error}");
            return 2;
        }
    };
    print_status(&value, &cli.url);
    0
}

fn print_status(value: &Value, url: &str) {
    let service = &value["service"];
    println!(
        "Service: {} (v{}, pid {}, uptime {}s)",
        url,
        service["version"].as_str().unwrap_or("?"),
        service["pid"].as_u64().unwrap_or(0),
        service["uptime_secs"].as_u64().unwrap_or(0)
    );
    println!(
        "Aggregate state: {}",
        value["aggregate"].as_str().unwrap_or("none")
    );
    println!(
        "Active sessions: {}",
        value["sessions"].as_array().map(Vec::len).unwrap_or(0)
    );
    let devices = value["devices"].as_array().cloned().unwrap_or_default();
    let connected = devices
        .iter()
        .filter(|device| device["connected"].as_bool().unwrap_or(false))
        .count();
    println!("Devices: {}/{} connected", connected, devices.len());
}

async fn devices(cli: &Cli) -> i32 {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        eprintln!("failed to create HTTP client");
        return 2;
    };
    let url = format!("{}/api/v1/devices", cli.url.trim_end_matches('/'));
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            eprintln!("failed to read devices (HTTP {})", response.status());
            return 2;
        }
        Err(error) => {
            eprintln!("cannot connect to agent-beacon ({}): {error}", cli.url);
            return 2;
        }
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("failed to parse device list: {error}");
            return 2;
        }
    };
    let devices = value.as_array().cloned().unwrap_or_default();
    if devices.is_empty() {
        println!("No supported devices found");
        return 0;
    }
    println!("{:<28} {:<14} {:<10} {}", "ID", "PORT", "PROFILE", "STATUS");
    for device in devices {
        println!(
            "{:<28} {:<14} {:<10} {}",
            device["id"].as_str().unwrap_or("?"),
            device["port"].as_str().unwrap_or("?"),
            device["profile"].as_str().unwrap_or("?"),
            if device["connected"].as_bool().unwrap_or(false) {
                format!("connected · {}", device["effect"].as_str().unwrap_or("off"))
            } else {
                "disconnected".to_string()
            }
        );
    }
    0
}

async fn hooks(cli: &Cli, args: HooksArgs) -> i32 {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        eprintln!("failed to create HTTP client");
        return 2;
    };
    let limit = args.limit.clamp(1, 1000);
    let url = format!(
        "{}/api/v1/hooks?limit={limit}",
        cli.url.trim_end_matches('/')
    );
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            eprintln!("failed to read hooks (HTTP {})", response.status());
            return 2;
        }
        Err(error) => {
            eprintln!("cannot connect to agent-beacon ({}): {error}", cli.url);
            return 2;
        }
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("failed to parse hook log: {error}");
            return 2;
        }
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
        );
        return 0;
    }

    let entries = value["entries"].as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        println!("No hook records");
        return 0;
    }
    println!(
        "{:<6} {:<9} {:<16} {:<24} {:<8} {:<9} {:<20} {:<16} DETAIL",
        "SEQ", "AGE", "KIND", "AGENT/SESSION", "STATE", "OUTCOME", "AGGREGATE", "EFFECT"
    );
    let now = now_ms();
    for entry in entries {
        let at_ms = entry["at_ms"].as_u64().unwrap_or(now);
        let age = format_age(now.saturating_sub(at_ms));
        let agent = entry["agent"].as_str().unwrap_or("");
        let session = entry["session"].as_str().unwrap_or("");
        let target = if agent.is_empty() && session.is_empty() {
            "-".to_string()
        } else {
            format!("{agent}/{session}")
        };
        let aggregate = transition_label(
            entry["aggregate_before"].as_str().unwrap_or("?"),
            entry["aggregate_after"].as_str().unwrap_or("?"),
        );
        let effect = transition_label(
            entry["effect_before"].as_str().unwrap_or("?"),
            entry["effect_after"].as_str().unwrap_or("?"),
        );
        let detail = entry["detail"]
            .as_str()
            .or_else(|| entry["error"].as_str())
            .unwrap_or("");
        println!(
            "{:<6} {:<9} {:<16} {:<24} {:<8} {:<9} {:<20} {:<16} {}",
            entry["seq"].as_u64().unwrap_or(0),
            age,
            entry["kind"].as_str().unwrap_or("?"),
            target,
            entry["state"].as_str().unwrap_or("-"),
            entry["outcome"].as_str().unwrap_or("?"),
            aggregate,
            effect,
            detail
        );
    }
    0
}

fn format_age(age_ms: u64) -> String {
    if age_ms < 1_000 {
        format!("{age_ms}ms")
    } else if age_ms < 60_000 {
        format!("{:.1}s", age_ms as f64 / 1_000.0)
    } else if age_ms < 3_600_000 {
        format!("{:.1}m", age_ms as f64 / 60_000.0)
    } else {
        format!("{:.1}h", age_ms as f64 / 3_600_000.0)
    }
}

fn transition_label(before: &str, after: &str) -> String {
    if before == after {
        before.to_string()
    } else {
        format!("{before}->{after}")
    }
}

fn parse_targets(raw: Option<&str>) -> Result<Vec<Target>, String> {
    let value = raw.unwrap_or("all").trim();
    match value {
        "" | "all" => Ok(Target::ALL
            .iter()
            .copied()
            .filter(|target| target.detected())
            .collect()),
        other => Target::parse(other)
            .map(|target| vec![target])
            .ok_or_else(|| {
                format!(
                    "unknown integration {other}; expected {} or all",
                    Target::ids()
                )
            }),
    }
}

fn integrate(args: IntegrateArgs, dir: Option<&std::path::Path>) -> i32 {
    match args.action {
        IntegrateAction::List => {
            println!(
                "{:<12} {:<14} {:<14} {}",
                "AGENT", "INSTALLED", "BUNDLED", "STATE"
            );
            let mut code = 0;
            for target in Target::ALL {
                match integrations::status(target, dir) {
                    Ok(status) => {
                        println!(
                            "{:<12} {:<14} {:<14} {}",
                            status.target.id(),
                            status.installed_version.as_deref().unwrap_or("-"),
                            status.embedded_version,
                            status.state.label()
                        );
                        println!("             {}", status.dir.display());
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
                eprintln!("--dir can only be used with a single target");
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
                eprintln!("--dir can only be used with a single target");
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
                    eprintln!("unknown integration {target}; expected {}", Target::ids());
                    return 2;
                }
            };
            match integrations::uninstall(target, dir) {
                Ok(path) => {
                    println!("{}: uninstalled {}", target.label(), path.display());
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
