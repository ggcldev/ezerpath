import { createSignal, For, Show, Resource, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import AnimatedNumber from "../components/AnimatedNumber";

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

function formatDate(raw: string): string {
  if (!raw) return "-";
  const d = new Date(raw);
  if (isNaN(d.getTime())) return raw;
  const m = String(d.getMonth() + 1).padStart(2, "0");
  return `${m}/${d.getDate()}/${String(d.getFullYear()).slice(2)}`;
}

const COLS = ["Posted", "Title", "Keyword", "Source", "Pay", "Link"];
const DEFAULT_WIDTHS = [80, 220, 110, 80, 100, 56];
const STAR_W = 32;

export default function WatchlistView(props: WatchlistViewProps) {
  const [filter, setFilter] = createSignal("");
  const [widths, setWidths] = createSignal<number[]>([...DEFAULT_WIDTHS]);

  let headerEl!: HTMLDivElement;
  let bodyEl!: HTMLDivElement;
  let drag = { active: false, i: 0, startX: 0, startW: 0 };

  const totalWidth = () => widths().reduce((a, b) => a + b, 0) + STAR_W;

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
    const q = filter().toLowerCase();
    const filtered = q
      ? list.filter((j) =>
          j.title.toLowerCase().includes(q) ||
          j.company.toLowerCase().includes(q) ||
          j.keyword.toLowerCase().includes(q))
      : list;
    return [...filtered].sort((a, b) => {
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
      if (!["onlinejobs.ph", "www.onlinejobs.ph"].includes(parsed.hostname)) return;
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

  return (
    <div class="flex-1 flex flex-col min-h-0 min-w-0 bg-mk-bg">
      {/* Titlebar */}
      <div
        class="h-14 shrink-0 flex items-end px-3 sm:px-5 pb-0"
        onMouseDown={handleWindowDrag}
      >
        <div class="flex items-center justify-between w-full">
          <div class="flex items-baseline gap-2">
            <h2 class="text-[15px] font-semibold text-mk-text">Watchlist</h2>
            <AnimatedNumber value={watchlistedJobs().length} class="text-[12px] text-mk-tertiary" />
          </div>
          <input
            class="w-40 sm:w-52 max-w-[48vw] px-2.5 py-1 text-[12px] rounded-md bg-mk-fill border border-mk-separator text-mk-text outline-none focus:border-mk-green focus:ring-2 focus:ring-mk-green-dim placeholder-mk-tertiary transition-all"
            type="text" placeholder="Filter..."
            value={filter()} onInput={(e) => setFilter(e.currentTarget.value)}
          />
        </div>
      </div>

      {/* Fixed header — outside scroll area */}
      <div ref={headerEl!} class="shrink-0 min-w-0 overflow-hidden px-3 sm:px-5 pt-3" style={{ background: "var(--mk-bg)" }}>
        <div class="flex items-center border-b border-mk-separator pb-1" style={{ width: `${totalWidth()}px` }}>
          {/* Star col */}
          <div style={{ width: `${STAR_W}px`, "min-width": `${STAR_W}px` }} />
          {/* Data cols */}
          <For each={COLS}>
            {(label, getI) => (
              <div
                class="relative text-[11px] font-semibold text-mk-secondary uppercase tracking-wider px-2 select-none"
                style={{ width: `${widths()[getI()]}px`, "min-width": `${widths()[getI()]}px` }}
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
        <table style={{ "table-layout": "fixed", "border-collapse": "collapse", width: `${totalWidth()}px` }}>
          <colgroup>
            <col style={{ width: `${STAR_W}px` }} />
            <For each={widths()}>{(w) => <col style={{ width: `${w}px` }} />}</For>
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
                    class={`border-b border-mk-separator/50 hover:bg-mk-row-hover transition-colors ${
                      rowIndex() % 2 === 1 ? "bg-mk-row-alt" : ""
                    }`}
                  >
                    <td class="text-center py-2.5">
                      <button
                        class="text-[15px] leading-none text-mk-yellow hover:opacity-80 transition-opacity"
                        aria-label="Remove from watchlist"
                        onClick={() => props.onToggleWatchlist(job.id)}
                      >{"\u2605"}</button>
                    </td>
                    <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[12px] text-mk-secondary">{formatDate(job.posted_at)}</span></td>
                    <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[13px] font-medium text-mk-text">{job.title}</span></td>
                    <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate"><span class="px-1.5 py-0.5 rounded text-[11px] bg-mk-fill text-mk-cyan border border-mk-separator">{job.keyword}</span></span></td>
                    <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[12px] text-mk-tertiary">{job.source}</span></td>
                    <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[13px] text-mk-secondary">{job.pay || "-"}</span></td>
                    <td class="text-center py-2.5">
                      <button class="px-2 py-0.5 text-[11px] rounded-md text-mk-cyan hover:bg-mk-fill transition-all" onClick={() => openUrl(job.url)}>Open</button>
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
