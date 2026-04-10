# TODO Review Backlog

## High Priority
- [x] Fix partial run inconsistency when crawl fails mid-loop.
  - Files: `app/src-tauri/src/lib.rs`
  - Add run lifecycle status (`running/succeeded/failed`) and always finalize totals/error on failure.
- [x] Fix Python spider `days` parameter being ignored in request URL.
  - Files: `crawler/spiders/onlinejobs.py`
  - Use parsed `days` in `dateposted` URL param (with validation/mapping).
- [x] Remove global text-selection lock.
  - Files: `app/src/App.css`
  - Remove `user-select: none` from global root and scope only to drag/resize handles.
- [x] Validate scraped URLs before storing/opening.
  - Files: `app/src-tauri/src/crawler/mod.rs`, `app/src/views/JobsView.tsx`, `app/src/views/WatchlistView.tsx`
  - Allow only safe schemes (`https`) and allowlisted hosts.

## Medium Priority
- [x] Fix `days_ago` SQL filter robustness.
  - Files: `app/src-tauri/src/db/mod.rs`
  - Parameterize query and compare using `julianday(...)` or numeric epoch storage.
- [x] Add backend guard against concurrent crawl invocations.
  - Files: `app/src-tauri/src/lib.rs`
  - Add mutex/state gate to prevent overlapping scans.
- [x] Add error handling + user feedback for frontend mutating actions.
  - Files: `app/src/App.tsx`, `app/src/views/ScanView.tsx`
  - Wrap `invoke` calls in `try/catch`, show UI error state, avoid false optimistic refresh.
- [x] Add confirmation for `Clear all`.
  - Files: `app/src/components/Sidebar.tsx`
  - Require confirmation (and optionally undo).
- [x] Memoize heavy derived computations in Jobs view.
  - Files: `app/src/views/JobsView.tsx`
  - Convert derived filter/group/count computations to `createMemo`.
- [x] Re-enable CSP with restrictive policy.
  - Files: `app/src-tauri/tauri.conf.json`
  - Replace `csp: null` with explicit production-safe policy.

## Low Priority
- [x] Add accessibility labels for icon-only controls.
  - Files: `app/src/views/JobsView.tsx`, `app/src/views/WatchlistView.tsx`, `app/src/components/Sidebar.tsx`
  - Add `aria-label` and preserve focus-visible styles.
- [x] Move dark-theme class toggle side-effect into `createEffect`.
  - Files: `app/src/App.tsx`
- [x] Make `run_daily.sh` portable (remove hardcoded `$HOME/ezerpath`).
  - Files: `run_daily.sh`
- [x] Pin Python dependencies for reproducible installs.
  - Files: `requirements.txt`
- [x] Replace recoverable `unwrap/expect` paths with propagated errors.
  - Files: `app/src-tauri/src/lib.rs`, `app/src-tauri/src/db/mod.rs`, `app/src-tauri/src/crawler/mod.rs`

## Testing and CI Gaps
- [x] Add CI workflow (`.github/workflows/ci.yml`) for:
  - Rust: `cargo check`, `cargo test`, `cargo clippy -D warnings`
  - Frontend: `npm ci`, `npm run build`, typecheck
  - Python crawler lint/test
- [x] Add backend integration tests:
  - Duplicate jobs in new run update `run_id` correctly
  - Deterministic `days_ago` filtering across timestamp formats/timezones
  - URL validation rejects unsafe schemes/hosts
- [x] Add spider tests:
  - `days` affects generated request URL
  - Date parsing/cutoff behavior
  - Pagination stop behavior on old posts
- [x] Add frontend behavior tests:
  - Rejected `invoke` paths show error UI and avoid state desync
  - Latest-scan count/list consistency
  - Clear-all confirmation requirement
