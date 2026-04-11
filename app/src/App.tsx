import { createSignal, createResource, createEffect, onCleanup, Match, Switch, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import Sidebar, { type View, type ScanRun } from "./components/Sidebar";
import ConfirmModal from "./components/ConfirmModal";
import ScanView from "./views/ScanView";
import JobsView from "./views/JobsView";
import WatchlistView from "./views/WatchlistView";
import { runMutation } from "./utils/mutations";
import toast, { Toaster } from "solid-toast";
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

interface ConfirmDialogState {
  title: string;
  description: string;
  confirmText: string;
  destructive?: boolean;
  onConfirm: () => Promise<void>;
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
  const [confirmDialog, setConfirmDialog] = createSignal<ConfirmDialogState | null>(null);
  const [confirmBusy, setConfirmBusy] = createSignal(false);

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
    await runMutation(
      () => invoke("toggle_watchlist", { jobId }),
      bump,
      setGlobalError
    );
  };

  const handleDeleteRun = async (runId: number) => {
    const ok = await runMutation(
      () => invoke("delete_run", { runId }),
      bump,
      setGlobalError
    );
    if (ok) {
      toast.success("Scan deleted.");
    } else {
      toast.error("Failed to delete scan.");
    }
  };

  const handleClearAll = async () => {
    const ok = await runMutation(
      () => invoke("clear_all_jobs"),
      bump,
      setGlobalError
    );
    if (ok) {
      refetchKeywords();
      refetchRuns();
      toast.success("Cleared all jobs and scan history.");
    } else {
      toast.error("Failed to clear jobs.");
    }
  };

  const requestDeleteRun = (run: ScanRun) => {
    const label = new Date(run.started_at).toLocaleString("en-US", {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    });
    setConfirmDialog({
      title: "Delete this scan?",
      description: `This will remove the ${label} scan and all jobs attached to it.`,
      confirmText: "Delete scan",
      destructive: true,
      onConfirm: async () => handleDeleteRun(run.id),
    });
  };

  const requestClearAll = () => {
    setConfirmDialog({
      title: "Clear all jobs?",
      description: "This will remove all scan history and all saved jobs. This action cannot be undone.",
      confirmText: "Clear all",
      destructive: true,
      onConfirm: handleClearAll,
    });
  };

  const handleConfirmModal = async () => {
    const dialog = confirmDialog();
    if (!dialog || confirmBusy()) return;
    setConfirmBusy(true);
    try {
      await dialog.onConfirm();
      setConfirmDialog(null);
    } finally {
      setConfirmBusy(false);
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
        onRequestDeleteRun={requestDeleteRun}
        onRequestClearAll={requestClearAll}
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
      <ConfirmModal
        open={!!confirmDialog()}
        title={confirmDialog()?.title ?? ""}
        description={confirmDialog()?.description ?? ""}
        confirmText={confirmDialog()?.confirmText ?? "Confirm"}
        destructive={!!confirmDialog()?.destructive}
        busy={confirmBusy()}
        onCancel={() => !confirmBusy() && setConfirmDialog(null)}
        onConfirm={handleConfirmModal}
      />
      <Toaster
        position="top-right"
        gutter={8}
        toastOptions={{
          duration: 3200,
          style: {
            "background-color": "var(--mk-grouped-bg)",
            color: "var(--mk-text)",
            border: "1px solid var(--mk-separator)",
            "font-size": "12px",
          },
        }}
      />
    </div>
  );
}

export default App;
