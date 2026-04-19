import { createSignal, For, Show, Resource, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check } from "lucide-solid";
import JobDetailsDrawer from "../components/JobDetailsDrawer";
import { rowHoverEnter, rowHoverLeave } from "../utils/fluidHover";
import { animateViewEnter } from "../utils/viewMotion";

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
  applied: boolean;
  job_type: string;
}

interface WatchlistViewProps {
  jobs: Resource<Job[]>;
  onToggleWatchlist: (jobId: number) => void;
  onToggleApplied: (jobId: number) => void;
}

function formatDate(raw: string): string {
  if (!raw) return "-";
  const d = new Date(raw);
  if (isNaN(d.getTime())) return raw;
  const m = String(d.getMonth() + 1).padStart(2, "0");
  return `${m}/${d.getDate()}/${String(d.getFullYear()).slice(2)}`;
}

const COLS = ["", "Posted", "Title", "Keyword", "Source", "Pay", "Type", "Link"];
const DEFAULT_WIDTHS = [26, 86, 330, 110, 80, 100, 90, 56];
const STAR_W = 32;

export default function WatchlistView(props: WatchlistViewProps) {
  const [widths, setWidths] = createSignal<number[]>([...DEFAULT_WIDTHS]);
  const [selectedJob, setSelectedJob] = createSignal<Job | null>(null);

  let headerEl!: HTMLDivElement;
  let bodyEl!: HTMLDivElement;
  let viewEl!: HTMLDivElement;
  let drag = { active: false, i: 0, startX: 0, startW: 0 };

  const totalWidth = () => widths().reduce((a, b) => a + b, 0) + STAR_W;
  const stretchedWidth = () => `max(100%, ${totalWidth()}px)`;

  const onBodyScroll = () => { headerEl.scrollLeft = bodyEl.scrollLeft; };

  const onMove = (e: MouseEvent) => {
    if (!drag.active) return;
    setWidths((prev) => {
      const next = [...prev];
      next[drag.i] = Math.max(50, drag.startW + e.clientX - drag.startX);
      return next;
    });
  };
  const onUp = () => {
    drag.active = false;
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  };
  onCleanup(() => {
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  });
  const startResize = (i: number, e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    drag = { active: true, i, startX: e.clientX, startW: widths()[i] };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const watchlistedJobs = () => {
    const list = (props.jobs() || []).filter((j) => j.watchlisted);
    return [...list].sort((a, b) => {
      const da = new Date(a.posted_at).getTime();
      const db = new Date(b.posted_at).getTime();
      return (isNaN(db) ? 0 : db) - (isNaN(da) ? 0 : da);
    });
  };
  const hasRows = () => (props.jobs() || []).length > 0;

  const openUrl = (rawUrl: string) => {
    try {
      const parsed = new URL(rawUrl);
      if (parsed.protocol !== "https:") return;
      const allowedHosts = ["onlinejobs.ph", "www.onlinejobs.ph", "bruntworkcareers.co", "www.bruntworkcareers.co"];
      if (!allowedHosts.includes(parsed.hostname)) return;
      invoke("plugin:opener|open_url", { url: parsed.toString() });
    } catch {
      // Ignore invalid URLs.
    }
  };
  const handleWindowDrag = (e: MouseEvent) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("button,input,a,textarea,select,[role='button']")) return;
    void getCurrentWindow().startDragging();
  };

  onMount(() => {
    animateViewEnter(viewEl);
  });

  return (
    <div ref={viewEl!} class="flex-1 flex flex-col min-h-0 min-w-0 bg-mk-bg">
      {/* Titlebar */}
      <div
        class="h-8 shrink-0"
        onMouseDown={handleWindowDrag}
      />

      <div class="relative flex flex-1 min-h-0">
        <div class="flex-1 flex flex-col min-h-0">
          {/* Fixed header — outside scroll area */}
          <div ref={headerEl!} class="shrink-0 min-w-0 overflow-hidden px-3 sm:px-5 pt-1" style={{ background: "var(--mk-grouped-bg)" }}>
            <div class="flex items-center border-b border-mk-separator pb-1" style={{ width: stretchedWidth() }}>
              {/* Star col */}
              <div style={{ width: `${STAR_W}px`, "min-width": `${STAR_W}px` }} />
              {/* Data cols */}
              <For each={COLS}>
                {(label, getI) => (
                  <div
                    class="relative text-left text-[11px] font-semibold text-mk-secondary uppercase tracking-wider px-2 pr-4 select-none overflow-hidden whitespace-nowrap"
                    style={getI() === COLS.length - 1
                      ? { "min-width": `${widths()[getI()]}px`, flex: "1 1 auto", "text-align": "left" }
                      : { width: `${widths()[getI()]}px`, "min-width": `${widths()[getI()]}px`, "text-align": "left" }}
                  >
                    {label}
                    <div
                      style={{
                        position: "absolute", right: "0", top: "0",
                        width: "8px", height: "100%",
                        cursor: "col-resize",
                        display: "flex", "align-items": "center", "justify-content": "center",
                      }}
                      on:mousedown={(e: MouseEvent) => startResize(getI(), e)}
                    >
                      <div style={{ width: "2px", height: "12px", "border-radius": "1px", background: "var(--mk-separator)" }} />
                    </div>
                  </div>
                )}
              </For>
            </div>
          </div>

          {/* Scrollable body */}
          <div ref={bodyEl!} class="flex-1 min-w-0 overflow-auto px-3 sm:px-5" onScroll={onBodyScroll}>
            <table style={{ "table-layout": "fixed", "border-collapse": "collapse", width: stretchedWidth() }}>
              <colgroup>
                <col style={{ width: `${STAR_W}px` }} />
                <For each={widths()}>
                  {(w, i) => i() === widths().length - 1
                    ? <col />
                    : <col style={{ width: `${w}px` }} />}
                </For>
              </colgroup>
              <tbody>
                <Show
                  when={!props.jobs.loading || hasRows()}
                  fallback={<tr><td colspan="7" class="text-center py-16 text-[13px] text-mk-tertiary">Loading...</td></tr>}
                >
                  <For
                    each={watchlistedJobs()}
                    fallback={
                      <tr>
                        <td colspan="7" class="text-center py-16">
                          <p class="text-[13px] text-mk-tertiary">No saved jobs</p>
                          <p class="text-[12px] text-mk-tertiary/60 mt-1">Star a job to add it here.</p>
                        </td>
                      </tr>
                    }
                  >
                    {(job, rowIndex) => (
                      <tr
                        class={`table-row cursor-pointer border-b border-mk-separator/50 hover:bg-mk-row-hover ${
                          rowIndex() % 2 === 1 ? "bg-mk-row-alt" : ""
                        }`}
                        onClick={() => setSelectedJob(job)}
                        onMouseEnter={(e) => rowHoverEnter(e.currentTarget)}
                        onMouseLeave={(e) => rowHoverLeave(e.currentTarget)}
                      >
                        <td
                          class={`text-center py-2.5 cursor-default ${job.applied ? "opacity-40 grayscale" : ""}`}
                          onClick={(e) => e.stopPropagation()}
                        >
                          <button
                            class="text-[15px] leading-none text-mk-yellow hover:opacity-80 transition-opacity"
                            aria-label="Remove from watchlist"
                            onClick={(e) => { e.stopPropagation(); props.onToggleWatchlist(job.id); }}
                          >{"\u2605"}</button>
                        </td>
                        <td
                          class="text-center py-2.5 px-1 cursor-default"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <button
                            class={`flex items-center justify-center w-[16px] h-[16px] mx-auto rounded-full border-[1.5px] transition-all duration-300 ease-[cubic-bezier(0.34,1.56,0.64,1)] hover:scale-110 active:scale-50 ${
                              job.applied
                                ? "bg-mk-green border-mk-green text-[#121212]"
                                : "bg-transparent border-mk-tertiary/40 text-transparent hover:border-mk-green/80 hover:bg-mk-green/10 hover:text-mk-green/60"
                            }`}
                            title={job.applied ? "Applied" : "Mark as applied"}
                            onClick={(e) => { e.stopPropagation(); props.onToggleApplied(job.id); }}
                          >
                            <Check size={10} strokeWidth={3.5} />
                          </button>
                        </td>
                        <td class={`px-2 py-2.5 overflow-hidden ${job.applied ? "opacity-40 grayscale" : ""}`}><span class="block truncate text-[12px] text-mk-secondary">{formatDate(job.posted_at)}</span></td>
                        <td class={`px-2 py-2.5 overflow-hidden ${job.applied ? "opacity-40 grayscale" : ""}`}><span class="block truncate text-[13px] font-medium text-mk-text">{job.title}</span></td>
                        <td class={`px-2 py-2.5 overflow-hidden ${job.applied ? "opacity-40 grayscale" : ""}`}><span class="block truncate"><span class="px-1.5 py-0.5 rounded text-[11px] bg-mk-fill text-mk-cyan border border-mk-separator">{job.keyword}</span></span></td>
                        <td class={`px-2 py-2.5 overflow-hidden ${job.applied ? "opacity-40 grayscale" : ""}`}><span class="block truncate text-[12px] text-mk-tertiary">{job.source}</span></td>
                        <td class={`px-2 py-2.5 overflow-hidden ${job.applied ? "opacity-40 grayscale" : ""}`}><span class="block truncate text-[13px] text-mk-secondary">{job.pay || "Undisclosed"}</span></td>
                        <td class={`px-2 py-2.5 overflow-hidden ${job.applied ? "opacity-40 grayscale" : ""}`}>
                          <Show when={job.job_type} fallback={<span class="text-mk-tertiary text-[11px]">-</span>}>
                            <span class={`inline-block px-1.5 py-0.5 rounded text-[10px] font-medium ${job.job_type.toLowerCase().includes("full") ? "bg-blue-500/15 text-blue-400 border border-blue-500/30" : job.job_type.toLowerCase().includes("part") ? "bg-amber-500/15 text-amber-400 border border-amber-500/30" : "bg-mk-fill text-mk-secondary border border-mk-separator"}`}>{job.job_type}</span>
                          </Show>
                        </td>
                        <td class={`px-2 py-2.5 overflow-hidden ${job.applied ? "opacity-40 grayscale" : ""}`}>
                          <button class="py-0.5 text-[11px] rounded-md text-mk-cyan hover:bg-mk-fill transition-all" onClick={(e) => { e.stopPropagation(); openUrl(job.url); }}>Open</button>
                        </td>
                      </tr>
                    )}
                  </For>
                </Show>
              </tbody>
            </table>
          </div>
        </div>
        <Show when={selectedJob()}>
          <button
            class="absolute inset-0 z-20 bg-black/20 backdrop-blur-[2px] animate-overlay-in"
            aria-label="Close job details"
            onClick={() => setSelectedJob(null)}
          />
        </Show>
        <JobDetailsDrawer
          job={selectedJob()}
          onClose={() => setSelectedJob(null)}
          onOpenUrl={openUrl}
        />
      </div>
    </div>
  );
}
