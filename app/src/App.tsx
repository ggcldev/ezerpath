import { createSignal, createResource, Match, Switch } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import Sidebar, { type View } from "./components/Sidebar";
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

  const toggleTheme = () => {
    const next = !dark();
    setDark(next);
    document.documentElement.classList.toggle("dark", next);
  };

  // Apply initial theme
  document.documentElement.classList.toggle("dark", dark());

  const bump = () => setVersion((v) => v + 1);

  const [jobs] = createResource(
    () => version(),
    () => invoke<Job[]>("get_jobs", { keyword: null, watchlistedOnly: false })
  );

  const [keywords, { refetch: refetchKeywords }] = createResource(
    () => version(),
    () => invoke<string[]>("get_keywords")
  );

  const sources = () => ["OnlineJobs.ph"];

  const handleToggleWatchlist = async (jobId: number) => {
    await invoke("toggle_watchlist", { jobId });
    bump();
  };

  return (
    <div class="h-screen flex bg-mk-bg">
      <Sidebar
        currentView={view()}
        onNavigate={setView}
        sources={sources()}
        crawling={crawling()}
        dark={dark()}
        onToggleTheme={toggleTheme}
      />
      <main class="flex-1 flex flex-col min-h-0 overflow-hidden">
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
              onScanComplete={() => { bump(); refetchKeywords(); }}
            />
          </Match>
          <Match when={view() === "jobs"}>
            <JobsView jobs={jobs} onToggleWatchlist={handleToggleWatchlist} />
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
