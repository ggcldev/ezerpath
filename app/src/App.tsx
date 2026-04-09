import { createSignal, createResource, For, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
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

type Tab = "all" | "watchlist";

function App() {
  const [tab, setTab] = createSignal<Tab>("all");
  const [crawling, setCrawling] = createSignal(false);
  const [crawlResult, setCrawlResult] = createSignal<CrawlStats[] | null>(null);
  const [filter, setFilter] = createSignal("");
  const [jobVersion, setJobVersion] = createSignal(0);

  const fetchJobs = async () => {
    jobVersion();
    const watchOnly = tab() === "watchlist";
    return await invoke<Job[]>("get_jobs", { keyword: null, watchlistedOnly: watchOnly });
  };

  const [jobs, { refetch }] = createResource(() => [tab(), jobVersion()], fetchJobs);
  const [keywords] = createResource(() => invoke<string[]>("get_keywords"));

  const filteredJobs = () => {
    const list = jobs() || [];
    const q = filter().toLowerCase();
    if (!q) return list;
    return list.filter(
      (j) =>
        j.title.toLowerCase().includes(q) ||
        j.company.toLowerCase().includes(q) ||
        j.keyword.toLowerCase().includes(q)
    );
  };

  const handleCrawl = async () => {
    setCrawling(true);
    setCrawlResult(null);
    try {
      const stats = await invoke<CrawlStats[]>("crawl_jobs");
      setCrawlResult(stats);
      setJobVersion((v) => v + 1);
    } catch (e) {
      console.error("Crawl failed:", e);
    }
    setCrawling(false);
  };

  const handleToggleWatchlist = async (jobId: number) => {
    await invoke("toggle_watchlist", { jobId });
    setJobVersion((v) => v + 1);
  };

  const openUrl = (url: string) => {
    invoke("plugin:opener|open_url", { url });
  };

  return (
    <main class="app">
      <header class="header">
        <h1 class="title">Ezerpath</h1>
        <p class="subtitle">Job Hunter Dashboard</p>
      </header>

      <div class="toolbar">
        <div class="tabs">
          <button
            class={tab() === "all" ? "tab active" : "tab"}
            onClick={() => setTab("all")}
          >
            All Jobs
          </button>
          <button
            class={tab() === "watchlist" ? "tab active" : "tab"}
            onClick={() => setTab("watchlist")}
          >
            Watchlist
          </button>
        </div>

        <input
          class="search"
          type="text"
          placeholder="Filter by title, company, keyword..."
          value={filter()}
          onInput={(e) => setFilter(e.currentTarget.value)}
        />

        <button class="crawl-btn" onClick={handleCrawl} disabled={crawling()}>
          {crawling() ? "Scanning..." : "Scan Now"}
        </button>
      </div>

      <Show when={crawlResult()}>
        <div class="crawl-stats">
          <For each={crawlResult()!}>
            {(s) => (
              <span class="stat-badge">
                {s.keyword}: {s.new} new / {s.found} found
              </span>
            )}
          </For>
        </div>
      </Show>

      <div class="table-wrap">
        <table class="job-table">
          <thead>
            <tr>
              <th class="col-star"></th>
              <th class="col-title">Title</th>
              <th class="col-company">Company</th>
              <th class="col-pay">Pay</th>
              <th class="col-keyword">Keyword</th>
              <th class="col-date">Posted</th>
              <th class="col-action"></th>
            </tr>
          </thead>
          <tbody>
            <Show when={!jobs.loading} fallback={<tr><td colspan="7" class="loading">Loading...</td></tr>}>
              <For each={filteredJobs()} fallback={<tr><td colspan="7" class="empty">No jobs found</td></tr>}>
                {(job) => (
                  <tr class={job.is_new ? "row new" : "row"}>
                    <td class="col-star">
                      <button
                        class={job.watchlisted ? "star active" : "star"}
                        onClick={() => handleToggleWatchlist(job.id)}
                        title={job.watchlisted ? "Remove from watchlist" : "Add to watchlist"}
                      >
                        {job.watchlisted ? "\u2605" : "\u2606"}
                      </button>
                    </td>
                    <td class="col-title">
                      <span class="job-title">{job.title}</span>
                      <Show when={job.is_new}><span class="new-badge">NEW</span></Show>
                    </td>
                    <td class="col-company">{job.company || "-"}</td>
                    <td class="col-pay">{job.pay || "-"}</td>
                    <td class="col-keyword"><span class="keyword-tag">{job.keyword}</span></td>
                    <td class="col-date">{job.posted_at.slice(0, 10) || "-"}</td>
                    <td class="col-action">
                      <button class="view-btn" onClick={() => openUrl(job.url)}>View</button>
                    </td>
                  </tr>
                )}
              </For>
            </Show>
          </tbody>
        </table>
      </div>

      <footer class="footer">
        <span>{filteredJobs().length} jobs</span>
        <Show when={keywords()}>
          <span>Keywords: {keywords()!.join(", ")}</span>
        </Show>
      </footer>
    </main>
  );
}

export default App;
