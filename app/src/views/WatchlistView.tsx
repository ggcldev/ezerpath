import { createSignal, For, Show, Resource } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

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

interface WatchlistViewProps {
  jobs: Resource<Job[]>;
  onToggleWatchlist: (jobId: number) => void;
}

export default function WatchlistView(props: WatchlistViewProps) {
  const [filter, setFilter] = createSignal("");

  const watchlistedJobs = () => {
    const list = (props.jobs() || []).filter((j) => j.watchlisted);
    const q = filter().toLowerCase();
    if (!q) return list;
    return list.filter(
      (j) =>
        j.title.toLowerCase().includes(q) ||
        j.company.toLowerCase().includes(q) ||
        j.keyword.toLowerCase().includes(q)
    );
  };

  const openUrl = (url: string) => {
    invoke("plugin:opener|open_url", { url });
  };

  return (
    <div class="flex-1 flex flex-col min-h-0 bg-mk-bg">
      <div class="h-12 shrink-0 flex items-end px-6 pb-0" data-tauri-drag-region>
        <div class="flex items-center justify-between w-full">
          <div class="flex items-baseline gap-2">
            <h2 class="text-[15px] font-semibold text-mk-text">Watchlist</h2>
            <span class="text-[12px] text-mk-tertiary">{watchlistedJobs().length}</span>
          </div>
          <input
            class="w-52 px-2.5 py-1 text-[12px] rounded-md bg-mk-fill border border-mk-separator text-mk-text outline-none focus:border-mk-green focus:ring-2 focus:ring-mk-green-dim placeholder-mk-tertiary transition-all"
            type="text"
            placeholder="Filter..."
            value={filter()}
            onInput={(e) => setFilter(e.currentTarget.value)}
          />
        </div>
      </div>

      <div class="flex-1 overflow-y-auto px-6 pt-3">
        <table class="w-full">
          <thead class="sticky top-0 z-10" style={{ background: "var(--mk-bg)" }}>
            <tr class="border-b border-mk-separator">
              <th class="w-8 py-2" />
              <th class="py-2 px-2 text-left text-[11px] font-semibold text-mk-tertiary uppercase tracking-wider" style="width:35%">Title</th>
              <th class="py-2 px-2 text-left text-[11px] font-semibold text-mk-tertiary uppercase tracking-wider" style="width:15%">Company</th>
              <th class="py-2 px-2 text-left text-[11px] font-semibold text-mk-tertiary uppercase tracking-wider" style="width:8%">Pay</th>
              <th class="py-2 px-2 text-left text-[11px] font-semibold text-mk-tertiary uppercase tracking-wider" style="width:10%">Source</th>
              <th class="py-2 px-2 text-left text-[11px] font-semibold text-mk-tertiary uppercase tracking-wider" style="width:12%">Keyword</th>
              <th class="py-2 px-2 text-left text-[11px] font-semibold text-mk-tertiary uppercase tracking-wider" style="width:10%">Posted</th>
              <th class="w-14 py-2" />
            </tr>
          </thead>
          <tbody>
            <Show
              when={!props.jobs.loading}
              fallback={<tr><td colspan="8" class="text-center py-16 text-[13px] text-mk-tertiary">Loading...</td></tr>}
            >
              <For
                each={watchlistedJobs()}
                fallback={
                  <tr>
                    <td colspan="8" class="text-center py-16">
                      <p class="text-[13px] text-mk-tertiary">No saved jobs</p>
                      <p class="text-[12px] text-mk-tertiary/60 mt-1">Star a job to add it here.</p>
                    </td>
                  </tr>
                }
              >
                {(job) => (
                  <tr class="border-b border-mk-separator/50 hover:bg-mk-fill transition-colors">
                    <td class="text-center">
                      <button
                        class="text-[15px] leading-none text-mk-yellow hover:opacity-80 transition-opacity"
                        onClick={() => props.onToggleWatchlist(job.id)}
                      >
                        {"\u2605"}
                      </button>
                    </td>
                    <td class="px-2 py-2 truncate text-[13px] font-medium text-mk-text">{job.title}</td>
                    <td class="px-2 py-2 truncate text-[13px] text-mk-secondary">{job.company || "-"}</td>
                    <td class="px-2 py-2 truncate text-[13px] text-mk-secondary">{job.pay || "-"}</td>
                    <td class="px-2 py-2 text-[12px] text-mk-tertiary">{job.source}</td>
                    <td class="px-2 py-2">
                      <span class="px-1.5 py-0.5 rounded text-[11px] bg-mk-fill text-mk-cyan border border-mk-separator">{job.keyword}</span>
                    </td>
                    <td class="px-2 py-2 text-[12px] text-mk-tertiary">{job.posted_at.slice(0, 10) || "-"}</td>
                    <td class="text-center">
                      <button
                        class="px-2 py-0.5 text-[11px] rounded-md text-mk-cyan hover:bg-mk-fill transition-all"
                        onClick={() => openUrl(job.url)}
                      >
                        Open
                      </button>
                    </td>
                  </tr>
                )}
              </For>
            </Show>
          </tbody>
        </table>
      </div>
    </div>
  );
}
