//! Headless WebView-based scraper for JS-rendered job pages.
//!
//! Instead of bundling a full headless browser (Playwright/Camoufox via
//! Python scrapling), we reuse the WebView that Tauri already ships with
//! the app (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux).
//!
//! Flow:
//!   1. Caller invokes `scrape(app, state, url, timeout)`.
//!   2. We allocate a unique request id, register a oneshot sender under
//!      that id in the shared pending map, and spawn a hidden
//!      WebviewWindow pointed at the target URL.
//!   3. An `initialization_script` is injected that runs in every frame
//!      of the page. It polls for content readiness (the page has
//!      non-trivial visible text and a main/article region has rendered)
//!      and then calls the Tauri command `scraper_webview_deliver` via
//!      `window.__TAURI_INTERNALS__.invoke`, passing the rendered HTML
//!      back to Rust.
//!   4. The `scraper_webview_deliver` command looks up the id, pops the
//!      oneshot sender, and forwards the payload. We close the hidden
//!      window and return the HTML.
//!   5. On timeout the window is closed and an error is returned.
//!
//! The bridge uses `__TAURI_INTERNALS__` which Tauri injects into every
//! webview, including those pointed at external URLs. The command is
//! gated by the capability configured in `capabilities/default.json`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

/// Payload delivered by the injected scraper script back to Rust.
#[derive(Clone, Debug, Serialize)]
pub struct ScrapeResult {
    pub html: String,
    pub text_length: usize,
    pub final_url: String,
}

/// Shared state: maps request-id → oneshot sender waiting for the result.
/// Held inside `AppState` so both the scraper fn and the delivery command
/// can access it.
#[derive(Default, Clone)]
pub struct WebviewScraperState {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ScrapeResult>>>>,
}

impl WebviewScraperState {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, id: String, tx: oneshot::Sender<ScrapeResult>) {
        if let Ok(mut map) = self.pending.lock() {
            map.insert(id, tx);
        }
    }

    fn take(&self, id: &str) -> Option<oneshot::Sender<ScrapeResult>> {
        self.pending.lock().ok()?.remove(id)
    }
}

/// Short unique id derived from a timestamp + random nibble. Avoids a `uuid`
/// dep for one use site.
fn new_request_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let rnd: u32 = rand_nibble();
    format!("{now:x}{rnd:x}")
}

/// Tiny pseudo-random used only for request-id disambiguation. Derived
/// from the low bits of the current nanoseconds; cryptographic quality
/// is not required because ids are process-local and short-lived.
fn rand_nibble() -> u32 {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    n.wrapping_mul(2_654_435_761) & 0xffff_ffff
}

/// Build the initialization script injected into the scraper webview.
///
/// Readiness is signalled by the combination of two conditions:
///   1. The DOM has been "idle" for `IDLE_MS` (no mutations observed).
///   2. The visible text (excluding <script>/<style>) exceeds `THRESHOLD`
///      AND contains at least one description-like marker word
///      ("responsibilities", "requirements", "qualifications", etc.).
///
/// This avoids delivering during the initial hydration pass, when
/// Next.js has rendered only the page shell and is still fetching the
/// job description via a secondary XHR. If neither condition is met by
/// `MAX_MS`, we deliver whatever we have and let Rust decide whether to
/// accept it or fall through to the next strategy in the chain.
fn build_init_script(request_id: &str, ready_text_threshold: usize) -> String {
    format!(
        r#"(function() {{
  const REQUEST_ID = {request_id:?};
  const THRESHOLD = {ready_text_threshold};
  const POLL_MS = 200;
  const IDLE_MS = 1500;
  const MAX_MS = 22000;
  const DESCRIPTION_MARKERS = [
    'responsibilities', 'requirements', 'qualifications', 'about the role',
    'about the job', 'what you\'ll do', 'what we offer', 'who you are',
    'key responsibilities', 'job description', 'duties'
  ];

  const startedAt = Date.now();
  let delivered = false;
  let lastMutationAt = Date.now();

  try {{
    const observer = new MutationObserver(() => {{ lastMutationAt = Date.now(); }});
    observer.observe(document.documentElement, {{
      childList: true, subtree: true, characterData: true, attributes: true
    }});
  }} catch (err) {{ /* ignore */ }}

  function visibleText() {{
    // innerText already excludes script/style contents; it reflects the
    // layout-rendered text the user would see.
    return (document.body && document.body.innerText) || '';
  }}

  function contentIsReady() {{
    const text = visibleText().toLowerCase();
    if (text.length < THRESHOLD) return false;
    return DESCRIPTION_MARKERS.some((m) => text.indexOf(m) !== -1);
  }}

  function deliver(reason) {{
    if (delivered) return;
    delivered = true;
    try {{
      const html = document.documentElement.outerHTML || '';
      const text = visibleText();
      const payload = {{
        id: REQUEST_ID,
        html,
        textLength: text.length,
        finalUrl: location.href,
      }};
      if (window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {{
        window.__TAURI_INTERNALS__.invoke('scraper_webview_deliver', payload);
      }}
    }} catch (err) {{
      // Best-effort: nothing we can do from here.
    }}
  }}

  function tick() {{
    if (delivered) return;
    const elapsed = Date.now() - startedAt;
    const idleFor = Date.now() - lastMutationAt;

    // Fast path: idle DOM + real content -> deliver immediately.
    if (idleFor >= IDLE_MS && contentIsReady()) {{
      deliver('idle+content');
      return;
    }}

    // Hard timeout: deliver regardless.
    if (elapsed >= MAX_MS) {{
      deliver('timeout');
      return;
    }}

    setTimeout(tick, POLL_MS);
  }}

  function boot() {{
    setTimeout(tick, POLL_MS);
  }}

  if (document.readyState === 'loading') {{
    document.addEventListener('DOMContentLoaded', boot, {{ once: true }});
  }} else {{
    boot();
  }}
}})();"#
    )
}

/// Render the given URL in a hidden WebviewWindow and return the final HTML.
///
/// This is the public entry point used by the crawler when a site (e.g.
/// Bruntwork) requires full JS execution to surface its content.
///
/// On success, returns the outerHTML of the rendered document. On timeout
/// or error, returns `Err`. The caller is expected to have a fallback
/// strategy (static HTML parse or the scrapling HTTP service).
pub async fn scrape<R: Runtime>(
    app: &AppHandle<R>,
    state: &WebviewScraperState,
    url: &str,
    timeout: Duration,
) -> Result<ScrapeResult, String> {
    let parsed_url = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("invalid url {url}: {e}"))?;

    let id = new_request_id();
    let label = format!("scraper-{id}");
    let init_script = build_init_script(&id, 200);

    let (tx, rx) = oneshot::channel();
    state.register(id.clone(), tx);

    // Build the hidden webview window. We intentionally disable file drop
    // handlers and shrink the window; visible=false keeps it offscreen.
    let build_result = {
        let label = label.clone();
        let init_script = init_script.clone();
        let app_clone = app.clone();
        let url_clone = parsed_url.clone();
        // Window creation must happen on the main thread on macOS; the
        // recommended pattern is `run_on_main_thread`.
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        app.run_on_main_thread(move || {
            let result = WebviewWindowBuilder::new(
                &app_clone,
                label,
                WebviewUrl::External(url_clone),
            )
            .title("ezerpath-scraper")
            .visible(false)
            .inner_size(800.0, 600.0)
            .skip_taskbar(true)
            .focused(false)
            .initialization_script(init_script)
            .build()
            .map(|_| ())
            .map_err(|e| format!("webview build failed: {e}"));
            let _ = ready_tx.send(result);
        })
        .map_err(|e| format!("run_on_main_thread failed: {e}"))?;
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| format!("webview creation channel: {e}"))?
    };

    if let Err(e) = build_result {
        // Remove the pending entry so it doesn't leak.
        let _ = state.take(&id);
        return Err(e);
    }

    // Wait for delivery or timeout.
    let outcome = tokio::time::timeout(timeout, rx).await;

    // Close the hidden window regardless of outcome. Do it on the main
    // thread to be safe across platforms.
    {
        let app_clone = app.clone();
        let label = label.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = app_clone.get_webview_window(&label) {
                let _ = w.destroy();
            }
        });
    }

    match outcome {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => {
            let _ = state.take(&id);
            Err("scraper channel dropped before delivery".to_string())
        }
        Err(_) => {
            let _ = state.take(&id);
            Err(format!(
                "webview scrape timed out after {}ms for {url}",
                timeout.as_millis()
            ))
        }
    }
}

/// Strip all `<script>` and `<style>` elements (and their contents) from a
/// captured HTML document. The downstream parser uses `scraper`, whose
/// `ElementRef::text()` treats `<script>` and `<style>` descendants as
/// plain text nodes, so RSC bootstrap chunks like
/// `self.__next_f.push([1,"..."])` would otherwise bleed into the
/// extracted description. Removing them before parsing is the simplest
/// robust fix and is cheap — webview-captured HTML is typically <1 MiB.
///
/// Implemented as a hand-rolled scanner rather than a regex to avoid
/// pulling in the regex crate for a single use site, and because
/// correctness against malformed markup matters less than speed here
/// (the content passes through two additional sanitizers downstream).
pub fn strip_scripts_and_styles(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Look for a '<' that starts <script or <style (case-insensitive).
        if bytes[i] == b'<' {
            let rest = &html[i..];
            let lower_prefix_len = rest.len().min(8);
            let lower_prefix = rest[..lower_prefix_len].to_ascii_lowercase();
            let (tag, end_tag) = if lower_prefix.starts_with("<script") {
                ("script", "</script")
            } else if lower_prefix.starts_with("<style") {
                ("style", "</style")
            } else {
                out.push('<');
                i += 1;
                continue;
            };
            let _ = tag; // kept for clarity
            // Skip past the opening tag's closing '>'.
            if let Some(open_end_rel) = rest.find('>') {
                let after_open = i + open_end_rel + 1;
                // Find the end tag (case-insensitive).
                let lower_rest = html[after_open..].to_ascii_lowercase();
                if let Some(close_rel) = lower_rest.find(end_tag) {
                    let close_abs = after_open + close_rel;
                    // Skip past the '>' of the closing tag too.
                    if let Some(final_end_rel) = html[close_abs..].find('>') {
                        i = close_abs + final_end_rel + 1;
                        continue;
                    }
                }
            }
            // Malformed — bail out and keep the rest as-is.
            out.push_str(&html[i..]);
            return out;
        } else {
            // Copy a run of non-'<' bytes in one shot.
            let next_lt = html[i..].find('<').map(|p| i + p).unwrap_or(html.len());
            out.push_str(&html[i..next_lt]);
            i = next_lt;
        }
    }
    out
}

/// Tauri command invoked from the injected scraper script. Delivers the
/// rendered page payload to the waiting Rust task.
#[tauri::command]
pub fn scraper_webview_deliver(
    state: tauri::State<'_, WebviewScraperState>,
    id: String,
    html: String,
    #[allow(non_snake_case)] textLength: Option<usize>,
    #[allow(non_snake_case)] finalUrl: Option<String>,
) -> Result<(), String> {
    let sender = match state.take(&id) {
        Some(s) => s,
        None => {
            // Late delivery or duplicate; silently ignore.
            return Ok(());
        }
    };
    let result = ScrapeResult {
        html,
        text_length: textLength.unwrap_or(0),
        final_url: finalUrl.unwrap_or_default(),
    };
    // Errors here mean the receiver was dropped; nothing we can do.
    let _ = sender.send(result);
    Ok(())
}
