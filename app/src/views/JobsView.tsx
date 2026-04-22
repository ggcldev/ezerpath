import { createSignal, createMemo, createResource, For, Show, Resource, Accessor, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import AnimatedNumber from "../components/AnimatedNumber";
import JobDetailsDrawer from "../components/JobDetailsDrawer";
import { getLatestRunId } from "../utils/jobs";
import { rowHoverEnter, rowHoverLeave } from "../utils/fluidHover";
import { openAllowlistedHttpsUrl } from "../utils/safeOpenUrl";
import { animateViewEnter } from "../utils/viewMotion";
import type { Job, JobFilterOptions, ScanRun } from "../types/ipc";

interface JobsViewProps {
  runs: Resource<ScanRun[]>;
  jobsRefreshKey: Accessor<number>;
  dateRange: Accessor<number>;
  crawling: boolean;
  enabledSources: Accessor<string[]>;
  onToggleWatchlist: (jobId: number) => void;
}

type PayRangeKey = "all" | "lt5" | "5_8" | "8_11" | "11_15" | "15_plus" | "unspecified";
type ScanScopeKey = "all" | "latest";

function formatDate(raw: string): string {
  if (!raw) return "-";
  const d = new Date(raw);
  if (isNaN(d.getTime())) return raw;
  const m = String(d.getMonth() + 1).padStart(2, "0");
  return `${m}/${d.getDate()}/${String(d.getFullYear()).slice(2)}`;
}

const COLS = ["Posted", "Title", "Source", "Pay", "Schedule", "Link"];
const DEFAULT_WIDTHS = [96, 340, 80, 100, 90, 56];
const STAR_W = 32;
const GROUP_INDENT_W = 14;
const PAY_RANGES: { key: Exclude<PayRangeKey, "all">; label: string }[] = [
  { key: "lt5", label: "< $5/hr" },
  { key: "5_8", label: "$5-7.99/hr" },
  { key: "8_11", label: "$8-10.99/hr" },
  { key: "11_15", label: "$11-14.99/hr" },
  { key: "15_plus", label: "$15+/hr" },
  { key: "unspecified", label: "Undisclosed" },
];

export default function JobsView(props: JobsViewProps) {
  const [selectedKeyword, setSelectedKeyword] = createSignal<string | null>(null);
  const [selectedPayRange, setSelectedPayRange] = createSignal<PayRangeKey>("all");
  const [selectedScanScope, setSelectedScanScope] = createSignal<ScanScopeKey>("all");
  const [selectedSource, setSelectedSource] = createSignal<string | null>(null);
  const [selectedSchedule, setSelectedSchedule] = createSignal<string | null>(null);
  const [widths, setWidths] = createSignal<number[]>([...DEFAULT_WIDTHS]);
  const [selectedJob, setSelectedJob] = createSignal<Job | null>(null);

  let headerEl!: HTMLDivElement;
  let headerRowEl!: HTMLDivElement;
  let tableEl!: HTMLTableElement;
  let bodyEl!: HTMLDivElement;
  let viewEl!: HTMLDivElement;
  let drag = { active: false, i: 0, startX: 0, startW: 0, currentW: 0 };

  const leadColWidth = () => STAR_W + (selectedKeyword() === null ? GROUP_INDENT_W : 0);
  const totalWidth = () => widths().reduce((a, b) => a + b, 0) + leadColWidth();
  const stretchedWidth = () => `max(100%, ${totalWidth()}px)`;
  const onBodyScroll = () => { headerEl.scrollLeft = bodyEl.scrollLeft; };

  const onMove = (e: MouseEvent) => {
    if (!drag.active) return;
    const newW = Math.max(50, drag.startW + e.clientX - drag.startX);
    drag.currentW = newW;
    // Direct DOM — updates header and colgroup in the same frame, no reactive lag
    const headerCol = headerRowEl?.children[drag.i + 1] as HTMLElement | undefined;
    if (headerCol) { headerCol.style.width = `${newW}px`; headerCol.style.minWidth = `${newW}px`; }
    const col = tableEl?.querySelector<HTMLElement>(`colgroup col:nth-child(${drag.i + 2})`);
    if (col) col.style.width = `${newW}px`;
  };
  const onUp = () => {
    drag.active = false;
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    // Commit final width to reactive state
    setWidths((prev) => { const next = [...prev]; next[drag.i] = drag.currentW; return next; });
  };
  onCleanup(() => {
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  });
  const startResize = (i: number, e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    drag = { active: true, i, startX: e.clientX, startW: widths()[i], currentW: widths()[i] };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const sourceLabel = (src: string) =>
    src === "onlinejobs" ? "OnlineJobs.ph"
    : src === "bruntwork" ? "BruntWork Careers"
    : src;
  const sourceDotClass = (src: string) =>
    src === "bruntwork" ? "bg-mk-cyan" : "bg-mk-green";

  const latestRunId = () => {
    return getLatestRunId(props.runs() || []);
  };

  const [filterOptions] = createResource(
    () => [props.jobsRefreshKey(), props.dateRange()] as const,
    ([, days]) => invoke<JobFilterOptions>("get_job_filter_options", { daysAgo: days })
  );

  const [jobs] = createResource(
    () => [
      props.jobsRefreshKey(),
      props.dateRange(),
      selectedKeyword(),
      selectedPayRange(),
      selectedScanScope(),
      selectedSource(),
      selectedSchedule(),
      latestRunId(),
    ] as const,
    ([, days, keyword, payRange, scanScope, source, schedule, runId]) =>
      invoke<Job[]>("get_jobs", {
        keyword,
        watchlistedOnly: false,
        daysAgo: days,
        source,
        jobType: schedule,
        payRange: payRange === "all" ? null : payRange,
        runId: scanScope === "latest" ? runId : null,
      })
  );

  const scheduleList = createMemo(() => {
    return (filterOptions()?.schedules || [])
      .map((item) => ({ schedule: item.value, count: item.count }))
      .sort((a, b) => b.count - a.count);
  });

  const sourceList = createMemo(() => {
    const enabled = props.enabledSources();
    return (filterOptions()?.sources || [])
      .filter((item) => enabled.includes(item.value))
      .map((item) => ({ source: item.value, count: item.count }))
      .sort((a, b) => b.count - a.count);
  });

  const keywordList = createMemo(() => {
    return (filterOptions()?.keywords || [])
      .map((item) => ({ keyword: item.value, count: item.count }))
      .sort((a, b) => b.count - a.count);
  });

  const payRangeList = createMemo(() => {
    const map = new Map<Exclude<PayRangeKey, "all">, number>();
    for (const key of PAY_RANGES) map.set(key.key, 0);
    for (const item of filterOptions()?.pay_ranges || []) {
      if (map.has(item.value as Exclude<PayRangeKey, "all">)) {
        map.set(item.value as Exclude<PayRangeKey, "all">, item.count);
      }
    }
    return PAY_RANGES.map((r) => ({ ...r, count: map.get(r.key) ?? 0 }));
  });

  const sortJobs = (jobs: Job[]) =>
    [...jobs].sort((a, b) => {
      const da = new Date(a.posted_at).getTime();
      const db = new Date(b.posted_at).getTime();
      return (isNaN(db) ? 0 : db) - (isNaN(da) ? 0 : da);
    });

  const latestRunCount = () => {
    return filterOptions()?.latest_run_count ?? 0;
  };

  const visibleJobs = createMemo(() => {
    const list = jobs() || [];
    const kw = selectedKeyword();

    if (kw) {
      return [{ keyword: kw, jobs: sortJobs(list) }];
    }

    const map = new Map<string, Job[]>();
    for (const job of list) {
      const k = job.keyword || "Other";
      if (!map.has(k)) map.set(k, []);
      map.get(k)!.push(job);
    }
    return [...map.entries()]
      .map(([keyword, jobs]) => ({ keyword, jobs: sortJobs(jobs) }))
      .sort((a, b) => {
        const ta = new Date(a.jobs[0]?.posted_at ?? "").getTime();
        const tb = new Date(b.jobs[0]?.posted_at ?? "").getTime();
      return (isNaN(tb) ? 0 : tb) - (isNaN(ta) ? 0 : ta);
      });
  });

  const totalCount = createMemo(() => visibleJobs().reduce((s, g) => s + g.jobs.length, 0));
  const allJobsCount = createMemo(() => keywordList().reduce((sum, item) => sum + item.count, 0));
  const hasRows = createMemo(() => (jobs() || []).length > 0);

  const openUrl = (rawUrl: string) => {
    void openAllowlistedHttpsUrl(rawUrl);
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

      <div class="h-8 shrink-0" onMouseDown={handleWindowDrag} />

      {/* Scanning banner */}
      <Show when={props.crawling}>
        <div class="mx-4 mt-2 flex items-center gap-2.5 px-3.5 py-2 rounded-lg bg-mk-grouped-bg border border-mk-separator shrink-0">
          <div class="relative h-3.5 w-3.5 shrink-0">
            <div class="absolute inset-0 rounded-full border-[1.5px] border-mk-separator" />
            <div class="absolute inset-0 rounded-full border-[1.5px] border-mk-cyan border-t-transparent animate-spin" />
          </div>
          <p class="text-[12px] text-mk-secondary">Scanning — new jobs will appear as they're found</p>
        </div>
      </Show>

      {/* Main content: keyword panel + table */}
      <div class="relative flex flex-1 min-h-0">

        {/* Keyword side panel */}
        <div class="w-44 md:w-52 shrink-0 flex flex-col border-r border-mk-separator py-3 overflow-y-auto">
          <p class="px-3 mb-1.5 text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Keywords</p>

          {/* All */}
          <button
            class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors ${
              selectedKeyword() === null
                ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
            }`}
            onClick={() => setSelectedKeyword(null)}
          >
            <span class="text-[12px] font-medium truncate">All</span>
            <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">
              {allJobsCount()}
            </span>
          </button>

          <For each={keywordList()}>
            {(item) => (
              <button
                class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors ${
                  selectedKeyword() === item.keyword
                    ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                    : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
                }`}
                onClick={() => setSelectedKeyword(item.keyword)}
              >
                <span class="text-[12px] truncate">{item.keyword}</span>
                <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">
                  {item.count}
                </span>
              </button>
            )}
          </For>

          {/* Scan scope */}
          <div class="mt-3 pt-3 border-t border-mk-separator">
            <p class="px-3 mb-1.5 text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Scan</p>

            <button
              class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors w-full ${
                selectedScanScope() === "all"
                  ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                  : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
              }`}
              onClick={() => setSelectedScanScope("all")}
            >
              <span class="text-[12px] font-medium truncate">All scans</span>
              <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">
                <AnimatedNumber value={allJobsCount()} class="inline-block" />
              </span>
            </button>

            <button
              class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors w-full ${
                selectedScanScope() === "latest"
                  ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                  : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
              }`}
              onClick={() => setSelectedScanScope("latest")}
            >
              <span class="text-[12px] font-medium truncate">Latest scan</span>
              <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">
                <AnimatedNumber value={latestRunCount()} class="inline-block" />
              </span>
            </button>
          </div>

          {/* Pay ranges */}
          <div class="mt-3 pt-3 border-t border-mk-separator">
            <p class="px-3 mb-1.5 text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Pay (USD/hr)</p>

            <button
              class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors w-full ${
                selectedPayRange() === "all"
                  ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                  : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
              }`}
              onClick={() => setSelectedPayRange("all")}
            >
              <span class="text-[12px] font-medium truncate">All rates</span>
              <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">
                {allJobsCount()}
              </span>
            </button>

            <For each={payRangeList()}>
              {(item) => (
                <button
                  class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors w-full ${
                    selectedPayRange() === item.key
                      ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                      : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
                  }`}
                  onClick={() => setSelectedPayRange(item.key)}
                >
                  <span class="text-[12px] truncate">{item.label}</span>
                  <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">
                    {item.count}
                  </span>
                </button>
              )}
            </For>
          </div>

          {/* Schedule */}
          <div class="mt-3 pt-3 border-t border-mk-separator">
            <p class="px-3 mb-1.5 text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Schedule</p>
            <button
              class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors w-full ${
                selectedSchedule() === null
                  ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                  : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
              }`}
              onClick={() => setSelectedSchedule(null)}
            >
              <span class="text-[12px] font-medium truncate">All</span>
              <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">{allJobsCount()}</span>
            </button>
            <For each={scheduleList()}>
              {(item) => (
                <button
                  class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors w-full ${
                    selectedSchedule() === item.schedule
                      ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                      : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
                  }`}
                  onClick={() => setSelectedSchedule(selectedSchedule() === item.schedule ? null : item.schedule)}
                >
                  <span class="text-[12px] truncate">{item.schedule}</span>
                  <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">{item.count}</span>
                </button>
              )}
            </For>
          </div>

          {/* Sources */}
          <div class="mt-3 pt-3 border-t border-mk-separator">
            <p class="px-3 mb-1.5 text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Sources</p>
            <button
              class={`flex items-center justify-between px-3 py-1.5 text-left transition-colors w-full ${
                selectedSource() === null
                  ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                  : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
              }`}
              onClick={() => setSelectedSource(null)}
            >
              <span class="text-[12px] font-medium">All</span>
              <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">{allJobsCount()}</span>
            </button>
            <For each={sourceList()}>
              {(item) => (
                <button
                  class={`flex items-center gap-2 px-3 py-1.5 text-left transition-colors w-full ${
                    selectedSource() === item.source
                      ? "text-mk-cyan bg-mk-fill border-l-2 border-mk-cyan"
                      : "text-mk-secondary hover:bg-mk-fill border-l-2 border-transparent"
                  }`}
                  onClick={() => setSelectedSource(selectedSource() === item.source ? null : item.source)}
                >
                  <span class={`w-[6px] h-[6px] rounded-full shrink-0 ${sourceDotClass(item.source)} ${
                    props.crawling && item.source === "onlinejobs" ? "animate-pulse" : ""
                  }`} />
                  <span class="text-[12px] truncate flex-1">{sourceLabel(item.source)}</span>
                  <span class="text-[11px] text-mk-green font-semibold ml-1 shrink-0">{item.count}</span>
                </button>
              )}
            </For>
          </div>
        </div>

        {/* Table area */}
        <div class="flex-1 flex flex-col min-h-0 min-w-0">

          {/* Fixed header */}
          <div ref={headerEl!} class="shrink-0 min-w-0 overflow-hidden px-3 sm:px-5 pt-1" style={{ background: "var(--mk-bg)" }}>
            <div ref={headerRowEl!} class="flex items-center border-b border-mk-separator pb-1" style={{ width: stretchedWidth() }}>
              <div style={{ width: `${leadColWidth()}px`, "min-width": `${leadColWidth()}px` }} />
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
            <table ref={tableEl!} style={{ "table-layout": "fixed", "border-collapse": "collapse", width: stretchedWidth() }}>
              <colgroup>
                <col style={{ width: `${leadColWidth()}px` }} />
                <For each={widths()}>
                  {(w, i) => i() === widths().length - 1
                    ? <col />
                    : <col style={{ width: `${w}px` }} />}
                </For>
              </colgroup>
              <tbody>
                <Show
                  when={!jobs.loading || hasRows()}
                  fallback={<tr><td colspan="6" class="text-center py-16 text-[13px] text-mk-tertiary">Loading...</td></tr>}
                >
                  <Show
                    when={totalCount() > 0}
                    fallback={<tr><td colspan="6" class="text-center py-16 text-[13px] text-mk-tertiary">No jobs yet</td></tr>}
                  >
                    <For each={visibleJobs()}>
                      {(group) => (
                        <>
                          {/* Group header — only shown when viewing All */}
                          <Show when={selectedKeyword() === null}>
                            <tr>
                              <td colspan="6" style={{ padding: "0" }}>
                                <div class="flex items-center gap-2 px-2 pt-4 pb-2" style={{ width: stretchedWidth() }}>
                                  <span class="text-[11px] font-semibold uppercase tracking-widest text-mk-cyan">{group.keyword}</span>
                                  <span class="text-[11px] text-mk-tertiary">{group.jobs.length}</span>
                                  <div class="flex-1 h-px" style={{ background: "var(--mk-separator)" }} />
                                </div>
                              </td>
                            </tr>
                          </Show>
                          <For each={group.jobs}>
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
                                  class={`text-center py-2.5 cursor-default ${selectedKeyword() === null ? "pl-3" : ""}`}
                                  onClick={(e) => e.stopPropagation()}
                                >
                                  <button
                                    class={`text-[15px] leading-none transition-colors ${job.watchlisted ? "text-mk-yellow" : "text-mk-tertiary hover:text-mk-yellow"}`}
                                    aria-label={job.watchlisted ? "Remove from watchlist" : "Add to watchlist"}
                                    onClick={(e) => { e.stopPropagation(); props.onToggleWatchlist(job.id); }}
                                  >{job.watchlisted ? "\u2605" : "\u2606"}</button>
                                </td>
                                <td class="px-2 py-2.5 overflow-hidden" style={{ "max-width": "0" }}><span class="block truncate text-[12px] text-mk-secondary">{formatDate(job.posted_at)}</span></td>
                                <td class="px-2 py-2.5 overflow-hidden" style={{ "max-width": "0" }}>
                                  <span class="block truncate text-[13px] font-medium text-mk-text">
                                    {job.title}
                                    <Show when={job.is_new}><span class="ml-1.5 px-1 py-px rounded text-[9px] font-bold bg-mk-green-dim text-mk-green">NEW</span></Show>
                                  </span>
                                </td>
                                <td class="px-2 py-2.5 overflow-hidden" style={{ "max-width": "0" }}><span class="block truncate text-[12px] text-mk-tertiary">{job.source}</span></td>
                                <td class="px-2 py-2.5 overflow-hidden" style={{ "max-width": "0" }}><span class="block truncate text-[13px] text-mk-secondary">{job.pay || "Undisclosed"}</span></td>
                                <td class="px-2 py-2.5 overflow-hidden" style={{ "max-width": "0" }}><span class="block truncate text-[12px] text-mk-secondary">{job.job_type || "-"}</span></td>
                                <td class="px-2 py-2.5 overflow-hidden">
                                  <button class="py-0.5 text-[11px] rounded-md text-mk-cyan hover:bg-mk-fill transition-all" onClick={(e) => { e.stopPropagation(); openUrl(job.url); }}>Open</button>
                                </td>
                              </tr>
                            )}
                          </For>
                        </>
                      )}
                    </For>
                  </Show>
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
