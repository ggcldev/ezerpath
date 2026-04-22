//! Auto-starts the legacy Python AI + scrapling fallback service on app launch.
//!
//! This is not a production runtime requirement after the 2026-04-22
//! `RETIRE SIDECAR` architecture decision. It remains temporarily so Phase 3
//! can remove fallback callers and startup diagnostics in a controlled patch.
//!
//! Behaviour:
//! - If a service is already responding at `127.0.0.1:8765/health`, do nothing.
//! - Otherwise, locate `ai_service/.venv/bin/uvicorn` relative to the project
//!   root (dev) or the executable dir and spawn it as a legacy fallback child.
//! - The spawned process is killed automatically when the returned handle is
//!   dropped (i.e. when the Tauri app shuts down).
//!
//! All startup events are written to `<log_dir>/ai_service.log` and mirrored
//! into an in-memory diagnostics snapshot. The UI can read the snapshot via
//! the `backend_diagnostics` IPC command when the service fails silently.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

const SERVICE_URL: &str = "http://127.0.0.1:8765/health";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(500);
const LOG_FILE_NAME: &str = "ai_service.log";

/// Handle to the spawned service. Kills the child on drop.
pub struct ServiceHandle {
    pub(crate) child: Option<std::process::Child>,
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Snapshot of the AI service's startup and runtime state. Exposed to the
/// frontend via the `backend_diagnostics` IPC command so users can see why
/// the service failed instead of the silent `eprintln!` sink we used before.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendStartupState {
    #[default]
    NotStarted,
    NotPackaged,
    Spawning,
    Ready,
    StartupFailed,
    TimedOut,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct BackendDiagnostics {
    pub state: BackendStartupState,
    pub service_url: String,
    pub reachable: bool,
    pub ready: bool,
    pub uvicorn_path: Option<String>,
    pub cwd: Option<String>,
    pub log_path: Option<String>,
    pub startup_error: Option<String>,
    pub child_pid: Option<u32>,
    pub started_at_ms: Option<u128>,
    pub ready_at_ms: Option<u128>,
    pub last_probe_at_ms: Option<u128>,
}

static DIAG: OnceLock<Mutex<BackendDiagnostics>> = OnceLock::new();
static LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn diag_cell() -> &'static Mutex<BackendDiagnostics> {
    DIAG.get_or_init(|| {
        Mutex::new(BackendDiagnostics {
            state: BackendStartupState::NotStarted,
            service_url: SERVICE_URL.into(),
            ..Default::default()
        })
    })
}

fn update_diag(f: impl FnOnce(&mut BackendDiagnostics)) {
    if let Ok(mut d) = diag_cell().lock() {
        f(&mut d);
    }
}

fn now_ms() -> Option<u128> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis())
}

/// Probe the service and return a fresh snapshot. Called by the
/// `backend_diagnostics` IPC command.
pub fn snapshot() -> BackendDiagnostics {
    let reachable = is_service_up();
    update_diag(|d| {
        d.reachable = reachable;
        d.last_probe_at_ms = now_ms();
        if reachable {
            d.ready = true;
            if d.ready_at_ms.is_none() {
                d.ready_at_ms = now_ms();
            }
            d.state = BackendStartupState::Ready;
        }
    });
    diag_cell()
        .lock()
        .map(|d| d.clone())
        .unwrap_or_default()
}

fn init_log(log_dir: &Path) {
    if std::fs::create_dir_all(log_dir).is_err() {
        return;
    }
    let path = log_dir.join(LOG_FILE_NAME);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let _ = LOG_FILE.set(Mutex::new(file));
    update_diag(|d| d.log_path = Some(path.to_string_lossy().into_owned()));
}

fn log_line(level: &str, msg: &str) {
    let ts_ms = now_ms().unwrap_or(0);
    let line = format!("{ts_ms} {level} [ai_service] {msg}\n");
    eprint!("{line}");
    if let Some(cell) = LOG_FILE.get() {
        if let Ok(mut guard) = cell.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
    }
}

fn record_error(state: BackendStartupState, msg: &str) {
    log_line("ERROR", msg);
    update_diag(|d| {
        d.state = state;
        d.startup_error = Some(msg.to_string());
    });
}

fn is_service_up() -> bool {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .and_then(|c| c.get(SERVICE_URL).send())
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Walk up from `start` looking for a directory that contains
/// `ai_service/.venv/bin/uvicorn`. Returns the path to uvicorn if found.
fn find_uvicorn(start: &Path) -> Option<PathBuf> {
    let mut dir: Option<&Path> = Some(start);
    while let Some(d) = dir {
        let candidate = d.join("ai_service").join(".venv").join("bin").join("uvicorn");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

fn spawn_log_pump<R: Read + Send + 'static>(level: &'static str, reader: R) {
    std::thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.trim().is_empty() => log_line(level, &line),
                Ok(_) => {}
                Err(err) => {
                    log_line(level, &format!("log pipe read failed: {err}"));
                    break;
                }
            }
        }
    });
}

fn wait_for_service_ready(
    timeout: Duration,
    poll_interval: Duration,
    mut checker: impl FnMut() -> bool,
) -> BackendStartupState {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if checker() {
            return BackendStartupState::Ready;
        }
        std::thread::sleep(poll_interval);
    }
    BackendStartupState::TimedOut
}

/// Start the service if not already running. Returns a handle that will
/// terminate the child on drop. If we cannot find the service binary,
/// returns an empty handle (app continues; crawler will fall back to
/// static HTML parsing).
pub fn start(log_dir: PathBuf) -> ServiceHandle {
    init_log(&log_dir);
    update_diag(|d| {
        d.state = BackendStartupState::Spawning;
        d.reachable = false;
        d.ready = false;
        d.started_at_ms = now_ms();
        d.ready_at_ms = None;
        d.last_probe_at_ms = None;
        d.startup_error = None;
        d.child_pid = None;
    });

    if is_service_up() {
        log_line("INFO", &format!("already running at {SERVICE_URL}"));
        update_diag(|d| {
            d.state = BackendStartupState::Ready;
            d.ready = true;
            d.reachable = true;
            d.ready_at_ms = now_ms();
        });
        return ServiceHandle { child: None };
    }

    let search_start = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let Some(uvicorn) = find_uvicorn(&search_start) else {
        record_error(BackendStartupState::NotPackaged, &format!(
            "uvicorn not found relative to {}. Scrapling fallback disabled.",
            search_start.display()
        ));
        return ServiceHandle { child: None };
    };
    update_diag(|d| d.uvicorn_path = Some(uvicorn.display().to_string()));

    // uvicorn path: <root>/ai_service/.venv/bin/uvicorn — cwd is <root>/ai_service
    let ai_service_dir = uvicorn
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());

    let Some(cwd) = ai_service_dir else {
        record_error(BackendStartupState::StartupFailed, &format!(
            "could not derive cwd from {}",
            uvicorn.display()
        ));
        return ServiceHandle { child: None };
    };
    update_diag(|d| d.cwd = Some(cwd.display().to_string()));

    log_line(
        "INFO",
        &format!(
            "spawning: {} server:app --host 127.0.0.1 --port 8765 (cwd={})",
            uvicorn.display(),
            cwd.display()
        ),
    );

    let child = Command::new(&uvicorn)
        .args(["server:app", "--host", "127.0.0.1", "--port", "8765"])
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => {
            update_diag(|d| d.child_pid = Some(c.id()));
            c
        }
        Err(e) => {
            record_error(BackendStartupState::StartupFailed, &format!("spawn failed: {e}"));
            return ServiceHandle { child: None };
        }
    };

    if let Some(stdout) = child.stdout.take() {
        spawn_log_pump("STDOUT", stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_pump("STDERR", stderr);
    }

    std::thread::spawn(|| match wait_for_service_ready(STARTUP_TIMEOUT, STARTUP_POLL_INTERVAL, is_service_up) {
        BackendStartupState::Ready => {
            log_line("INFO", &format!("ready at {SERVICE_URL}"));
            update_diag(|d| {
                d.state = BackendStartupState::Ready;
                d.ready = true;
                d.reachable = true;
                d.ready_at_ms = now_ms();
                d.last_probe_at_ms = now_ms();
            });
        }
        BackendStartupState::TimedOut => {
            record_error(
                BackendStartupState::TimedOut,
                &format!(
                    "service did not respond at {SERVICE_URL} within {}s",
                    STARTUP_TIMEOUT.as_secs()
                ),
            );
            update_diag(|d| d.last_probe_at_ms = now_ms());
        }
        _ => {}
    });

    ServiceHandle { child: Some(child) }
}

#[cfg(test)]
mod tests {
    use super::{wait_for_service_ready, BackendStartupState};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn wait_for_service_ready_returns_ready_before_timeout() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let seen = attempts.clone();
        let state = wait_for_service_ready(
            Duration::from_millis(50),
            Duration::from_millis(1),
            move || seen.fetch_add(1, Ordering::SeqCst) >= 2,
        );
        assert_eq!(state, BackendStartupState::Ready);
        assert!(attempts.load(Ordering::SeqCst) >= 3);
    }

    #[test]
    fn wait_for_service_ready_times_out_when_probe_never_succeeds() {
        let state = wait_for_service_ready(
            Duration::from_millis(10),
            Duration::from_millis(1),
            || false,
        );
        assert_eq!(state, BackendStartupState::TimedOut);
    }
}
