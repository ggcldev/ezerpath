import { createSignal, createResource, createEffect, onCleanup, Match, Switch, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import Sidebar, { type View, type ScanRun } from "./components/Sidebar";
import ScanView from "./views/ScanView";
import JobsView from "./views/JobsView";
import WatchlistView from "./views/WatchlistView";
import "./App.css";

interface Job {
  id: number;
  source: string;
  source_id: string;
  title: string;
  company: string;
  pay: string;
  posted_at: string;
  url: string;
  summary: string;
  keyword: string;
  scraped_at: string;
  is_new: boolean;
  watchlisted: boolean;
  run_id: number | null;
}

interface CrawlStats {
  keyword: string;
  found: number;
  new: number;
  pages: number;
}

function App() {
  const [view, setView] = createSignal<View>("scan");
  const [crawling, setCrawling] = createSignal(false);
  const [crawlResult, setCrawlResult] = createSignal<CrawlStats[] | null>(null);
  const [crawlError, setCrawlError] = createSignal("");
  const [version, setVersion] = createSignal(0);
  const [dark, setDark] = createSignal(true);
  const [dateRange, setDateRange] = createSignal<number>(3);
  const [globalError, setGlobalError] = createSignal("");

  const toggleTheme = () => {
    setDark((v) => !v);
  };

  createEffect(() => {
    document.documentElement.classList.toggle("dark", dark());
  });

  const bump = () => setVersion((v) => v + 1);

  createEffect(() => {
    if (!crawling()) return;
    const id = setInterval(() => bump(), 1500);
    onCleanup(() => clearInterval(id));
  });

  const [jobs] = createResource(
    () => [version(), dateRange()] as const,
    ([, days]) => invoke<Job[]>("get_jobs", { keyword: null, watchlistedOnly: false, daysAgo: days })
  );

  const [keywords, { refetch: refetchKeywords }] = createResource(
    () => version(),
    () => invoke<string[]>("get_keywords")
  );

  const [runs, { refetch: refetchRuns }] = createResource(
    () => version(),
    () => invoke<ScanRun[]>("get_runs")
  );

  const handleToggleWatchlist = async (jobId: number) => {
    try {
      await invoke("toggle_watchlist", { jobId });
      setGlobalError("");
      bump();
    } catch (e) {
      setGlobalError(String(e));
    }
  };

  const handleDeleteRun = async (runId: number) => {
    try {
      await invoke("delete_run", { runId });
      setGlobalError("");
      bump();
    } catch (e) {
      setGlobalError(String(e));
    }
  };

  const handleClearAll = async () => {
    try {
      await invoke("clear_all_jobs");
      setGlobalError("");
      bump();
    } catch (e) {
      setGlobalError(String(e));
    }
  };

  const handleScanStart = () => setView("jobs");

  return (
    <div class="h-screen flex bg-mk-bg">
      <Sidebar
        currentView={view()}
        onNavigate={setView}
        crawling={crawling()}
        dark={dark()}
        onToggleTheme={toggleTheme}
        runs={runs}
        onDeleteRun={handleDeleteRun}
        onClearAll={handleClearAll}
      />
      <main class="flex-1 flex flex-col min-h-0 min-w-0 overflow-hidden">
        <Show when={globalError()}>
          <div class="mx-4 mt-3 shrink-0 rounded-lg border border-mk-pink/30 bg-mk-grouped-bg px-3 py-2 flex items-center justify-between gap-3">
            <p class="text-[12px] text-mk-pink truncate">{globalError()}</p>
            <button
              class="text-[11px] text-mk-secondary hover:text-mk-text"
              aria-label="Dismiss error message"
              onClick={() => setGlobalError("")}
            >
              Dismiss
            </button>
          </div>
        </Show>
        <Switch>
          <Match when={view() === "scan"}>
            <ScanView
              crawling={crawling}
              setCrawling={setCrawling}
              crawlResult={crawlResult}
              setCrawlResult={setCrawlResult}
              crawlError={crawlError}
              setCrawlError={setCrawlError}
              keywords={keywords}
              dateRange={dateRange}
              setDateRange={setDateRange}
              onScanStart={handleScanStart}
              onScanComplete={() => { bump(); refetchKeywords(); refetchRuns(); }}
            />
          </Match>
          <Match when={view() === "jobs"}>
            <JobsView
              jobs={jobs}
              runs={runs}
              crawling={crawling()}
              onToggleWatchlist={handleToggleWatchlist}
            />
          </Match>
          <Match when={view() === "watchlist"}>
            <WatchlistView jobs={jobs} onToggleWatchlist={handleToggleWatchlist} />
          </Match>
        </Switch>
      </main>
    </div>
  );
}

export default App;
