# TODO Review Backlog

## High Priority
- [ ] Fix partial run inconsistency when crawl fails mid-loop.
  - Files: `app/src-tauri/src/lib.rs`
  - Add run lifecycle status (`running/succeeded/failed`) and always finalize totals/error on failure.
- [ ] Fix Python spider `days` parameter being ignored in request URL.
  - Files: `crawler/spiders/onlinejobs.py`
  - Use parsed `days` in `dateposted` URL param (with validation/mapping).
- [ ] Remove global text-selection lock.
  - Files: `app/src/App.css`
  - Remove `user-select: none` from global root and scope only to drag/resize handles.
- [ ] Validate scraped URLs before storing/opening.
  - Files: `app/src-tauri/src/crawler/mod.rs`, `app/src/views/JobsView.tsx`, `app/src/views/WatchlistView.tsx`
  - Allow only safe schemes (`https`) and allowlisted hosts.

## Medium Priority
- [ ] Fix `days_ago` SQL filter robustness.
  - Files: `app/src-tauri/src/db/mod.rs`
  - Parameterize query and compare using `julianday(...)` or numeric epoch storage.
- [ ] Add backend guard against concurrent crawl invocations.
  - Files: `app/src-tauri/src/lib.rs`
  - Add mutex/state gate to prevent overlapping scans.
- [ ] Add error handling + user feedback for frontend mutating actions.
  - Files: `app/src/App.tsx`, `app/src/views/ScanView.tsx`
  - Wrap `invoke` calls in `try/catch`, show UI error state, avoid false optimistic refresh.
- [ ] Add confirmation for `Clear all`.
  - Files: `app/src/components/Sidebar.tsx`
  - Require confirmation (and optionally undo).
- [ ] Memoize heavy derived computations in Jobs view.
  - Files: `app/src/views/JobsView.tsx`
  - Convert derived filter/group/count computations to `createMemo`.
- [ ] Re-enable CSP with restrictive policy.
  - Files: `app/src-tauri/tauri.conf.json`
  - Replace `csp: null` with explicit production-safe policy.

## Low Priority
- [ ] Add accessibility labels for icon-only controls.
  - Files: `app/src/views/JobsView.tsx`, `app/src/views/WatchlistView.tsx`, `app/src/components/Sidebar.tsx`
  - Add `aria-label` and preserve focus-visible styles.
- [ ] Move dark-theme class toggle side-effect into `createEffect`.
  - Files: `app/src/App.tsx`
- [ ] Make `run_daily.sh` portable (remove hardcoded `$HOME/ezerpath`).
  - Files: `run_daily.sh`
- [ ] Pin Python dependencies for reproducible installs.
  - Files: `requirements.txt`
- [ ] Replace recoverable `unwrap/expect` paths with propagated errors.
  - Files: `app/src-tauri/src/lib.rs`, `app/src-tauri/src/db/mod.rs`, `app/src-tauri/src/crawler/mod.rs`

## Testing and CI Gaps
- [ ] Add CI workflow (`.github/workflows/ci.yml`) for:
  - Rust: `cargo check`, `cargo test`, `cargo clippy -D warnings`
  - Frontend: `npm ci`, `npm run build`, typecheck
  - Python crawler lint/test
- [ ] Add backend integration tests:
  - Duplicate jobs in new run update `run_id` correctly
  - Deterministic `days_ago` filtering across timestamp formats/timezones
  - URL validation rejects unsafe schemes/hosts
- [ ] Add spider tests:
  - `days` affects generated request URL
  - Date parsing/cutoff behavior
  - Pagination stop behavior on old posts
- [ ] Add frontend behavior tests:
  - Rejected `invoke` paths show error UI and avoid state desync
  - Latest-scan count/list consistency
  - Clear-all confirmation requirement
