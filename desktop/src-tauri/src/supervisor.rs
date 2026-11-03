//! 子进程监管：附着/拉起 agent-beacon、解析 stdout 事件、splash 状态与退出清理。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::prefs::Prefs;

const MAIN: &str = "main";
const SPLASH: &str = "splash";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

static CHILD_PID: AtomicI32 = AtomicI32::new(0);

pub fn install_signal_handlers() {
    #[cfg(unix)]
    {
        extern "C" fn handle(_signal: libc::c_int) {
            let pid = CHILD_PID.load(Ordering::SeqCst);
            if pid > 0 {
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            unsafe { libc::_exit(0) }
        }
        unsafe {
            for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
                libc::signal(signal, handle as *const () as libc::sighandler_t);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Starting,
    Running,
    Stopped,
    Failed,
}

struct Inner {
    state: State,
    addr: Option<String>,
    detail: Option<String>,
    failure: Option<String>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    owned: bool,
    generation: u64,
}

pub struct Supervisor {
    port: u16,
    hidden: bool,
    log_path: PathBuf,
    inner: Mutex<Inner>,
    prefs: Mutex<Prefs>,
}

impl Supervisor {
    pub fn new(port: u16, hidden: bool, prefs: Prefs, log_path: PathBuf) -> Self {
        Self {
            port,
            hidden,
            log_path,
            inner: Mutex::new(Inner {
                state: State::Starting,
                addr: None,
                detail: None,
                failure: None,
                child: None,
                stdin: None,
                owned: false,
                generation: 0,
            }),
            prefs: Mutex::new(prefs),
        }
    }

    pub fn start(self: &Arc<Self>, app: &AppHandle) {
        if !self.hidden {
            self.ensure_splash(app);
        }
        {
            let mut inner = self.lock();
            inner.state = State::Starting;
            inner.addr = None;
            inner.detail = None;
            inner.failure = None;
        }
        self.inject_current(app);
        let supervisor = Arc::clone(self);
        let app = app.clone();
        std::thread::spawn(move || supervisor.run(&app));
    }

    fn run(self: &Arc<Self>, app: &AppHandle) {
        if health_ok(self.port) {
            self.mark_running(app, format!("127.0.0.1:{}", self.port), false);
            return;
        }

        let bin = match locate_helper(app) {
            Ok(path) => path,
            Err(error) => {
                self.fail(app, "missing-binary", error);
                return;
            }
        };

        let log = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            Ok(file) => file,
            Err(error) => {
                self.fail(
                    app,
                    "spawn-failed",
                    format!("打开日志 {} 失败：{error}", self.log_path.display()),
                );
                return;
            }
        };

        let mut command = Command::new(&bin);
        command
            .arg("serve")
            .arg("--supervised")
            .arg("--port")
            .arg(self.port.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.fail(
                    app,
                    "spawn-failed",
                    format!("启动 {} 失败：{error}", bin.display()),
                );
                return;
            }
        };

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let generation = {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.state = State::Starting;
            inner.addr = None;
            inner.detail = None;
            inner.failure = None;
            inner.child = Some(child);
            inner.stdin = stdin;
            inner.owned = true;
            inner.generation
        };
        CHILD_PID.store(
            self.lock()
                .child
                .as_ref()
                .map(|child| child.id() as i32)
                .unwrap_or(0),
            Ordering::SeqCst,
        );

        if let Some(stdout) = stdout {
            let supervisor = Arc::clone(self);
            let app = app.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    supervisor.handle_line(&app, generation, &line);
                }
            });
        }

        let started = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let (current, addr, exited) = {
                let mut inner = self.lock();
                let exited = match inner.child.as_mut() {
                    Some(child) => !matches!(child.try_wait(), Ok(None)),
                    None => true,
                };
                (inner.generation, inner.addr.clone(), exited)
            };
            if current != generation {
                return;
            }
            if let Some(addr) = addr {
                if health_ok_addr(&addr) {
                    self.mark_running(app, addr, true);
                }
            }
            if exited {
                let state = self.lock().state;
                if state != State::Running && state != State::Stopped {
                    self.fail(
                        app,
                        "crash",
                        format!("agent-beacon 已退出，日志：{}", self.log_path.display()),
                    );
                }
                return;
            }
            if started.elapsed() > HANDSHAKE_TIMEOUT && self.lock().addr.is_none() {
                self.fail(
                    app,
                    "timeout",
                    format!(
                        "等待 agent-beacon 就绪超时（{} 秒），日志：{}",
                        HANDSHAKE_TIMEOUT.as_secs(),
                        self.log_path.display()
                    ),
                );
                return;
            }
        }
    }

    fn handle_line(self: &Arc<Self>, app: &AppHandle, generation: u64, line: &str) {
        let Some(rest) = line.strip_prefix("agent-beacon: ") else {
            return;
        };
        if self.lock().generation != generation {
            return;
        }
        if let Some(addr) = rest.strip_prefix("listening addr=") {
            let addr = addr.trim().to_string();
            {
                let mut inner = self.lock();
                inner.addr = Some(addr.clone());
            }
            if health_ok_addr(&addr) {
                self.mark_running(app, addr, true);
            }
        } else if rest.starts_with("bind-failed") {
            self.fail(app, "bind-failed", format!("端口绑定失败：{rest}"));
        }
    }

    fn mark_running(self: &Arc<Self>, app: &AppHandle, addr: String, owned: bool) {
        {
            let mut inner = self.lock();
            if inner.state == State::Running {
                return;
            }
            inner.state = State::Running;
            inner.addr = Some(addr);
            inner.detail = None;
            inner.failure = None;
            inner.owned = owned;
        }
        self.inject_current(app);
        if !self.hidden {
            self.open_console(app);
        }
    }

    fn fail(self: &Arc<Self>, app: &AppHandle, kind: &str, detail: String) {
        {
            let mut inner = self.lock();
            inner.state = State::Failed;
            inner.failure = Some(kind.to_string());
            inner.detail = Some(detail);
        }
        self.kill_child();
        let supervisor = Arc::clone(self);
        let app = app.clone();
        let app_for_task = app.clone();
        let _ = app.run_on_main_thread(move || {
            supervisor.ensure_splash(&app_for_task);
            supervisor.inject_current(&app_for_task);
            if let Some(window) = app_for_task.get_webview_window(MAIN) {
                let _ = window.close();
            }
        });
    }

    pub fn open_console(self: &Arc<Self>, app: &AppHandle) {
        let addr = self.lock().addr.clone();
        let Some(addr) = addr else {
            self.ensure_splash(app);
            return;
        };
        let app = app.clone();
        let _ = app.clone().run_on_main_thread(move || {
            open_console_now(&app, &addr);
        });
    }

    pub fn restart(self: &Arc<Self>, app: &AppHandle) {
        self.kill_child();
        self.start(app);
    }

    pub fn stop(self: &Arc<Self>, app: &AppHandle) {
        self.kill_child();
        {
            let mut inner = self.lock();
            inner.state = State::Stopped;
            inner.addr = None;
            inner.detail = Some("服务已停止，可在托盘中选择「启动服务」重新启动。".to_string());
            inner.failure = None;
        }
        let supervisor = Arc::clone(self);
        let app = app.clone();
        let app_for_task = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = app_for_task.get_webview_window(MAIN) {
                let _ = window.close();
            }
            supervisor.ensure_splash(&app_for_task);
            supervisor.inject_current(&app_for_task);
        });
    }

    pub fn shutdown(&self) {
        self.kill_child();
    }

    pub fn lights_off(&self) {
        let Some(addr) = self.lock().addr.clone() else {
            return;
        };
        std::thread::spawn(move || {
            let _ = post_json(
                &addr,
                "/api/v1/control",
                r#"{"effect":"off","duration_secs":60}"#,
            );
        });
    }

    pub fn set_autostart(&self, app: &AppHandle, enabled: bool) -> Result<bool, String> {
        let manager = app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|error| format!("调整开机自启动失败：{error}"))?;
        let actual = manager.is_enabled().unwrap_or(enabled);
        {
            let mut prefs = self.prefs.lock().unwrap_or_else(|p| p.into_inner());
            prefs.autostart = actual;
            prefs.save(app)?;
        }
        Ok(actual)
    }

    fn ensure_splash(self: &Arc<Self>, app: &AppHandle) {
        if app.get_webview_window(SPLASH).is_some() {
            return;
        }
        let supervisor = Arc::clone(self);
        let built = WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App("index.html".into()))
            .title("AgentBeacon")
            .inner_size(560.0, 440.0)
            .resizable(false)
            .on_page_load(move |window, _| {
                supervisor.inject_to(&window);
            })
            .build();
        if let Ok(window) = built {
            let _ = window.center();
            self.inject_to(&window);
        }
    }

    fn inject_current(&self, app: &AppHandle) {
        if let Some(window) = app.get_webview_window(SPLASH) {
            self.inject_to(&window);
        }
    }

    fn inject_to(&self, window: &WebviewWindow) {
        let (state, addr, detail, failure) = {
            let inner = self.lock();
            (
                inner.state,
                inner.addr.clone(),
                inner.detail.clone(),
                inner.failure.clone(),
            )
        };
        let (status, hint) = match state {
            State::Starting => ("正在启动 agent-beacon…".to_string(), None),
            State::Running => (
                match addr {
                    Some(addr) => format!("运行中 · {addr}"),
                    None => "运行中".to_string(),
                },
                None,
            ),
            State::Stopped => (
                "服务已停止".to_string(),
                Some("可在托盘中选择「启动服务」。".to_string()),
            ),
            State::Failed => (
                "启动失败".to_string(),
                Some(failure_hint(failure.as_deref())),
            ),
        };
        let status = serde_json::to_string(&status).unwrap_or_else(|_| "\"\"".into());
        let hint = serde_json::to_string(&hint).unwrap_or_else(|_| "null".into());
        let detail = serde_json::to_string(&detail).unwrap_or_else(|_| "null".into());
        let failed = state == State::Failed;
        let _ = window.eval(format!(
            "window.agentBeaconSetStatus && window.agentBeaconSetStatus({status}, {hint}, {detail}, {failed});"
        ));
    }

    fn kill_child(&self) {
        let mut inner = self.lock();
        inner.generation += 1;
        inner.addr = None;
        inner.stdin.take();
        let child = inner.child.take();
        CHILD_PID.store(0, Ordering::SeqCst);
        drop(inner);
        if let Some(mut child) = child {
            terminate(&mut child);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

fn failure_hint(kind: Option<&str>) -> String {
    match kind {
        Some("missing-binary") => {
            "未找到随包分发的 agent-beacon。请重新安装，或确认安装包完整。".to_string()
        }
        Some("bind-failed") => {
            "端口已被占用。请释放端口后点击「重试」，或修改服务配置中的端口。".to_string()
        }
        Some("timeout") => "等待服务就绪超时。可查看日志后重试。".to_string(),
        Some("crash") => "服务进程异常退出。可查看日志后重试。".to_string(),
        _ => "请查看日志后重试，或从托盘退出应用。".to_string(),
    }
}

fn open_console_now(app: &AppHandle, addr: &str) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let Ok(url) = format!("http://{addr}/").parse::<Url>() else {
        return;
    };
    let opener = app.clone();
    let built = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(url))
        .title("AgentBeacon")
        .inner_size(1200.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .on_navigation(move |url| {
            let local = matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"));
            if !local {
                let _ = opener.opener().open_url(url.as_str(), None::<&str>);
            }
            local
        })
        .build();
    match built {
        Ok(window) => {
            if let Some(splash) = app.get_webview_window(SPLASH) {
                let _ = splash.close();
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(error) => {
            eprintln!("failed to open console: {error}");
        }
    }
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    for _ in 0..30 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn locate_helper(app: &AppHandle) -> Result<PathBuf, String> {
    let names: &[&str] = if cfg!(windows) {
        &["agent-beacon.exe", "agent-beacon"]
    } else {
        &["agent-beacon", "agent-beacon.exe"]
    };
    let mut tried = Vec::new();

    if let Ok(path) = std::env::var("AGENT_BEACON_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        tried.push(candidate.display().to_string());
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(found) = find_in(dir, names, &mut tried) {
                return Ok(found);
            }
        }
    }

    if let Ok(resource) = app.path().resource_dir() {
        if let Some(found) = find_in(&resource, names, &mut tried) {
            return Ok(found);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if let Some(found) = find_in(&dir, names, &mut tried) {
                return Ok(found);
            }
        }
    }

    Err(format!(
        "未找到 agent-beacon 可执行文件。已尝试：\n  - {}",
        tried.join("\n  - ")
    ))
}

fn find_in(dir: &Path, names: &[&str], tried: &mut Vec<String>) -> Option<PathBuf> {
    for name in names {
        let candidate = dir.join(name);
        tried.push(candidate.display().to_string());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn health_ok(port: u16) -> bool {
    health_ok_addr(&format!("127.0.0.1:{port}"))
}

fn health_ok_addr(addr: &str) -> bool {
    let Ok(socket) = addr.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&socket, Duration::from_millis(400)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(400)));
    let request =
        format!("GET /api/v1/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buffer = [0u8; 64];
    let Ok(read) = stream.read(&mut buffer) else {
        return false;
    };
    let head = String::from_utf8_lossy(&buffer[..read]);
    head.starts_with("HTTP/1.1 2") || head.starts_with("HTTP/1.0 2")
}

fn post_json(addr: &str, path: &str, body: &str) -> Result<(), String> {
    let socket: SocketAddr = addr
        .parse()
        .map_err(|error| format!("非法地址 {addr}：{error}"))?;
    let mut stream = TcpStream::connect_timeout(&socket, Duration::from_millis(800))
        .map_err(|error| format!("连接 {addr} 失败：{error}"))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("写入请求失败：{error}"))
}
