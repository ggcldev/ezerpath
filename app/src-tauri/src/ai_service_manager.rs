//! Auto-starts the bundled Python AI + scrapling service on app launch.
//!
//! Behaviour:
//! - If a service is already responding at `127.0.0.1:8765/health`, do nothing.
//! - Otherwise, locate `ai_service/.venv/bin/uvicorn` relative to the project
//!   root (dev) or the executable dir (production) and spawn it as a child.
//! - The spawned process is killed automatically when the returned handle is
//!   dropped (i.e. when the Tauri app shuts down).

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const SERVICE_URL: &str = "http://127.0.0.1:8765/health";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

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
fn find_uvicorn(start: &std::path::Path) -> Option<PathBuf> {
    let mut dir: Option<&std::path::Path> = Some(start);
    while let Some(d) = dir {
        let candidate = d.join("ai_service").join(".venv").join("bin").join("uvicorn");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

/// Start the service if not already running. Returns a handle that will
/// terminate the child on drop. If we cannot find the service binary,
/// returns an empty handle (app continues; crawler will fall back to
/// static HTML parsing).
pub fn start() -> ServiceHandle {
    if is_service_up() {
        eprintln!("[ai_service] already running at {SERVICE_URL}");
        return ServiceHandle { child: None };
    }

    // Search from the current exe's directory outward.
    let search_start = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let Some(uvicorn) = find_uvicorn(&search_start) else {
        eprintln!(
            "[ai_service] uvicorn not found relative to {}. \
             Scrapling fallback disabled.",
            search_start.display()
        );
        return ServiceHandle { child: None };
    };

    // The `ai_service` directory is uvicorn's parent's parent's parent.
    // uvicorn path: <root>/ai_service/.venv/bin/uvicorn
    let ai_service_dir = uvicorn
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());

    let Some(cwd) = ai_service_dir else {
        eprintln!("[ai_service] could not derive cwd from {}", uvicorn.display());
        return ServiceHandle { child: None };
    };

    eprintln!(
        "[ai_service] spawning: {} server:app --host 127.0.0.1 --port 8765 (cwd={})",
        uvicorn.display(),
        cwd.display()
    );

    let child = Command::new(&uvicorn)
        .args(["server:app", "--host", "127.0.0.1", "--port", "8765"])
        .current_dir(&cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    let child = match child {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[ai_service] spawn failed: {e}");
            return ServiceHandle { child: None };
        }
    };

    // Wait up to STARTUP_TIMEOUT for the service to become reachable, so the
    // first crawl doesn't race the uvicorn boot.
    let deadline = std::time::Instant::now() + STARTUP_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if is_service_up() {
            eprintln!("[ai_service] ready at {SERVICE_URL}");
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    ServiceHandle { child }
}
