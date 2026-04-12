import { createSignal, createResource, createEffect, onCleanup, Match, Switch, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import Sidebar, { type View, type ScanRun } from "./components/Sidebar";
import ConfirmModal from "./components/ConfirmModal";
import SettingsPanel from "./components/SettingsPanel";
import ScanView from "./views/ScanView";
import JobsView from "./views/JobsView";
import WatchlistView from "./views/WatchlistView";
import EzerView from "./views/EzerView";
import { runMutation } from "./utils/mutations";
import toast, { Toaster } from "solid-toast";
import "./App.css";

interface Job {
  id: number;
  source: string;
  source_id: string;
  title: string;
  company: string;
  company_logo_url: string;
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

interface AiRuntimeConfig {
  ollama_base_url: string;
  ollama_model: string;
  embedding_service_url: string;
  embedding_model: string;
  temperature: number;
  max_tokens: number;
  timeout_ms: number;
}

interface EmbeddingIndexStatus {
  jobs_total: number;
  jobs_indexed: number;
  resumes_total: number;
  resumes_indexed: number;
  active_embedding_model: string;
}

interface ResumeProfile {
  id: number;
  name: string;
  source_file: string;
  raw_text: string;
  normalized_text: string;
  created_at: string;
  updated_at: string;
  is_active: boolean;
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
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [aiBusy, setAiBusy] = createSignal(false);
  const [ollamaStatus, setOllamaStatus] = createSignal("");
  const [embeddingStatus, setEmbeddingStatus] = createSignal("");
  const [indexStatus, setIndexStatus] = createSignal("");
  const [resumes, setResumes] = createSignal<ResumeProfile[]>([]);
  const [selectedResumeId, setSelectedResumeId] = createSignal<number | null>(null);
  const [resumeFilePath, setResumeFilePath] = createSignal("");
  const [resumeStatus, setResumeStatus] = createSignal("");
  const [aiConfig, setAiConfig] = createSignal<AiRuntimeConfig>({
    ollama_base_url: "http://127.0.0.1:11434",
    ollama_model: "qwen2.5:7b-instruct",
    embedding_service_url: "http://127.0.0.1:8765",
    embedding_model: "all-MiniLM-L6-v2",
    temperature: 0.2,
    max_tokens: 1024,
    timeout_ms: 30000,
  });

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

  const loadAiConfig = async () => {
    try {
      const cfg = await invoke<AiRuntimeConfig>("get_ai_runtime_config");
      setAiConfig(cfg);
    } catch (e: any) {
      setGlobalError(String(e));
    }
  };

  const loadResumes = async () => {
    try {
      const items = await invoke<ResumeProfile[]>("list_resumes");
      setResumes(items);
      const active = items.find((r) => r.is_active);
      if (active) setSelectedResumeId(active.id);
      else if (items.length > 0 && selectedResumeId() === null) setSelectedResumeId(items[0].id);
    } catch (e: any) {
      setGlobalError(String(e));
    }
  };

  const withAiBusy = async (fn: () => Promise<void>) => {
    if (aiBusy()) return;
    setAiBusy(true);
    try {
      await fn();
    } finally {
      setAiBusy(false);
    }
  };

  const handleOpenSettings = async () => {
    setSettingsOpen(true);
    await loadAiConfig();
    await loadResumes();
  };

  const saveAiConfig = () =>
    withAiBusy(async () => {
      await invoke("set_ai_runtime_config", { config: aiConfig() });
      toast.success("AI settings saved.");
    });

  const checkOllama = () =>
    withAiBusy(async () => {
      const health = await invoke<{ ok: boolean; message: string; model_count: number }>("ai_health_check");
      setOllamaStatus(`${health.message} Models: ${health.model_count}`);
      toast.success("Ollama check completed.");
    });

  const checkEmbedding = () =>
    withAiBusy(async () => {
      const health = await invoke<{ ok: boolean; message: string; model_name: string }>("ai_embedding_health_check");
      setEmbeddingStatus(`${health.message} Model: ${health.model_name}`);
      toast.success("Embedding service check completed.");
    });

  const indexJobs = () =>
    withAiBusy(async () => {
      const status = await invoke<EmbeddingIndexStatus>("index_jobs_embeddings");
      setIndexStatus(
        `Indexed ${status.jobs_indexed}/${status.jobs_total} jobs and ${status.resumes_indexed}/${status.resumes_total} resumes (${status.active_embedding_model}).`
      );
      toast.success("Job embeddings indexed.");
    });

  const uploadResumeFromPath = () =>
    withAiBusy(async () => {
      if (!resumeFilePath().trim()) {
        toast.error("Please enter a resume file path first.");
        return;
      }
      const profile = await invoke<ResumeProfile>("upload_resume_from_file", {
        filePath: resumeFilePath().trim(),
        displayName: null,
      });
      setResumeStatus(`Uploaded: ${profile.name}`);
      await loadResumes();
      setSelectedResumeId(profile.id);
      toast.success("Resume uploaded.");
    });

  const selectResume = (resumeId: number) => {
    if (!Number.isFinite(resumeId) || resumeId <= 0) return;
    setSelectedResumeId(resumeId);
    invoke("set_active_resume", { resumeId }).catch(() => {
      // do not interrupt UI flow; settings status can be refreshed manually
    });
  };

  const indexResume = () =>
    withAiBusy(async () => {
      if (!selectedResumeId()) {
        toast.error("Select a resume profile first.");
        return;
      }
      const status = await invoke<EmbeddingIndexStatus>("index_resume_embedding", { resumeId: selectedResumeId() });
      setResumeStatus(
        `Indexed resume. Jobs: ${status.jobs_indexed}/${status.jobs_total}, Resumes: ${status.resumes_indexed}/${status.resumes_total} (${status.active_embedding_model}).`
      );
      toast.success("Resume embedding indexed.");
    });

  return (
    <div class="h-screen flex bg-mk-bg">
      <Sidebar
        currentView={view()}
        onNavigate={setView}
        crawling={crawling()}
        dark={dark()}
        onToggleTheme={toggleTheme}
        onOpenSettings={handleOpenSettings}
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
          <Match when={view() === "ezer"}>
            <EzerView />
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
      <SettingsPanel
        open={settingsOpen()}
        dark={dark()}
        onToggleTheme={toggleTheme}
        aiConfig={aiConfig()}
        aiBusy={aiBusy()}
        ollamaStatus={ollamaStatus()}
        embeddingStatus={embeddingStatus()}
        indexStatus={indexStatus()}
        resumes={resumes().map((r) => ({ id: r.id, name: r.name, source_file: r.source_file, is_active: r.is_active }))}
        selectedResumeId={selectedResumeId()}
        resumeFilePath={resumeFilePath()}
        resumeStatus={resumeStatus()}
        onAiConfigChange={setAiConfig}
        onSaveAiConfig={saveAiConfig}
        onCheckOllama={checkOllama}
        onCheckEmbedding={checkEmbedding}
        onIndexJobs={indexJobs}
        onResumeFilePathChange={setResumeFilePath}
        onUploadResumeFromPath={uploadResumeFromPath}
        onSelectResume={selectResume}
        onIndexResume={indexResume}
        onClose={() => setSettingsOpen(false)}
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
