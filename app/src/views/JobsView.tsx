import { createSignal, For, Show, Resource, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ScanRun } from "../components/Sidebar";

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

interface JobsViewProps {
  jobs: Resource<Job[]>;
  runs: Resource<ScanRun[]>;
  crawling: boolean;
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

const COLS = ["Posted", "Title", "Keyword", "Source", "Pay", "Company", "Link"];
const DEFAULT_WIDTHS = [80, 220, 110, 80, 100, 130, 56];
const STAR_W = 32;
const GROUP_INDENT_W = 14;
const PAY_RANGES: { key: Exclude<PayRangeKey, "all">; label: string }[] = [
  { key: "lt5", label: "< $5/hr" },
  { key: "5_8", label: "$5-7.99/hr" },
  { key: "8_11", label: "$8-10.99/hr" },
  { key: "11_15", label: "$11-14.99/hr" },
  { key: "15_plus", label: "$15+/hr" },
  { key: "unspecified", label: "Unspecified/Negotiable" },
];
const PHP_PER_USD = 56;
const HOURS_PER_MONTH = 160;

function parsePayToUsdHourly(payRaw: string): number | null {
  const raw = (payRaw || "").trim();
  if (!raw) return null;

  const lower = raw.toLowerCase();
  if (/(tbd|tba|tbc|negotiable|neg\b|depends|open|to be discuss|willing to pay|ranges?)/.test(lower)) {
    return null;
  }

  const nums = [...lower.matchAll(/(\d[\d,]*(?:\.\d+)?)/g)]
    .map((m) => Number(m[1].replace(/,/g, "")))
    .filter((n) => Number.isFinite(n) && n > 0);
  if (nums.length === 0) return null;

  const isRange = nums.length >= 2 && /(-|–|to)/.test(lower);
  let amount = isRange ? (nums[0] + nums[1]) / 2 : nums[0];

  if (/(php|₱)/.test(lower)) amount /= PHP_PER_USD;

  const isHourly = /(\/\s*h|\/\s*hr|\/\s*hour|per\s*hour|\bhourly\b)/.test(lower);
  const isMonthly = /(\/\s*mo|\/\s*month|\bmonthly\b|\bmonth\b)/.test(lower);

  if (isHourly) return amount;
  if (isMonthly) return amount / HOURS_PER_MONTH;

  // OnlineJobs commonly mixes monthly and hourly values without explicit units.
  if (amount >= 80) return amount / HOURS_PER_MONTH;
  return amount;
}

function getPayRangeKey(payRaw: string): Exclude<PayRangeKey, "all"> {
  const hourly = parsePayToUsdHourly(payRaw);
  if (hourly === null) return "unspecified";
  if (hourly < 5) return "lt5";
  if (hourly < 8) return "5_8";
  if (hourly < 11) return "8_11";
  if (hourly < 15) return "11_15";
  return "15_plus";
}

export default function JobsView(props: JobsViewProps) {
  const [filter, setFilter] = createSignal("");
  const [selectedKeyword, setSelectedKeyword] = createSignal<string | null>(null);
  const [selectedPayRange, setSelectedPayRange] = createSignal<PayRangeKey>("all");
  const [selectedScanScope, setSelectedScanScope] = createSignal<ScanScopeKey>("all");
  const [widths, setWidths] = createSignal<number[]>([...DEFAULT_WIDTHS]);

  let headerEl!: HTMLDivElement;
  let bodyEl!: HTMLDivElement;
  let drag = { active: false, i: 0, startX: 0, startW: 0 };

  const leadColWidth = () => STAR_W + (selectedKeyword() === null ? GROUP_INDENT_W : 0);
  const totalWidth = () => widths().reduce((a, b) => a + b, 0) + leadColWidth();
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

  // Keyword list derived from ALL jobs (not filtered), so panel always shows everything
  const keywordList = () => {
    const list = props.jobs() || [];
    const map = new Map<string, number>();
    for (const job of list) {
      const kw = job.keyword || "Other";
      map.set(kw, (map.get(kw) ?? 0) + 1);
    }
    return [...map.entries()]
      .map(([keyword, count]) => ({ keyword, count }))
      .sort((a, b) => b.count - a.count);
  };

  // Pay ranges derived from all jobs, normalized to USD hourly equivalents.
  const payRangeList = () => {
    const list = props.jobs() || [];
    const map = new Map<Exclude<PayRangeKey, "all">, number>();
    for (const key of PAY_RANGES) map.set(key.key, 0);
    for (const job of list) {
      const key = getPayRangeKey(job.pay);
      map.set(key, (map.get(key) ?? 0) + 1);
    }
    return PAY_RANGES.map((r) => ({ ...r, count: map.get(r.key) ?? 0 }));
  };

  const sortJobs = (jobs: Job[]) =>
    [...jobs].sort((a, b) => {
      const da = new Date(a.posted_at).getTime();
      const db = new Date(b.posted_at).getTime();
      return (isNaN(db) ? 0 : db) - (isNaN(da) ? 0 : da);
    });

  const latestRunId = () => {
    const list = props.runs() || [];
    if (list.length === 0) return null;
    return list[0].id;
  };

  const latestRunCount = () => {
    const list = props.jobs() || [];
    const runId = latestRunId();
    if (runId === null) return 0;
    return list.filter((j) => j.run_id === runId).length;
  };

  // When a keyword is selected: flat filtered list. When All: grouped.
  const visibleJobs = () => {
    const list = props.jobs() || [];
    const q = filter().toLowerCase();
    const kw = selectedKeyword();
    const payRange = selectedPayRange();
    const scanScope = selectedScanScope();
    const runId = latestRunId();

    const baseByScope = scanScope === "latest" && runId !== null
      ? list.filter((j) => j.run_id === runId)
      : list;
    const baseByKeyword = kw ? baseByScope.filter((j) => (j.keyword || "Other") === kw) : baseByScope;
    const base = payRange === "all"
      ? baseByKeyword
      : baseByKeyword.filter((j) => getPayRangeKey(j.pay) === payRange);
    const searched = q
      ? base.filter((j) =>
          j.title.toLowerCase().includes(q) ||
          j.company.toLowerCase().includes(q) ||
          j.keyword.toLowerCase().includes(q))
      : base;

    if (kw) {
      // Flat sorted list for single keyword
      return [{ keyword: kw, jobs: sortJobs(searched) }];
    }

    // Group by keyword, sorted by most recent job
    const map = new Map<string, Job[]>();
    for (const job of searched) {
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
  };

  const totalCount = () => visibleJobs().reduce((s, g) => s + g.jobs.length, 0);

  const openUrl = (url: string) => invoke("plugin:opener|open_url", { url });
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
            <h2 class="text-[15px] font-semibold text-mk-text">All Jobs</h2>
            <span class="text-[12px] text-mk-tertiary">{totalCount()}</span>
          </div>
          <input
            class="w-40 sm:w-52 max-w-[48vw] px-2.5 py-1 text-[12px] rounded-md bg-mk-fill border border-mk-separator text-mk-text outline-none focus:border-mk-green focus:ring-2 focus:ring-mk-green-dim placeholder-mk-tertiary transition-all"
            type="text" placeholder="Filter..."
            value={filter()} onInput={(e) => setFilter(e.currentTarget.value)}
          />
        </div>
      </div>

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
      <div class="flex flex-1 min-h-0">

        {/* Keyword side panel */}
        <div class="w-44 md:w-52 shrink-0 flex flex-col border-r border-mk-separator py-3">
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
              {(props.jobs() || []).length}
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
                {(props.jobs() || []).length}
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
                {latestRunCount()}
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
                {(props.jobs() || []).length}
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

          {/* Sources */}
          <div class="mt-auto px-3 pt-4 pb-1 border-t border-mk-separator">
            <p class="text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2">Sources</p>
            <div class="flex items-center gap-2">
              <span class={`w-[6px] h-[6px] rounded-full shrink-0 transition-colors ${
                props.crawling ? "bg-mk-green animate-pulse" : "bg-mk-tertiary"
              }`} />
              <span class="text-[11px] text-mk-secondary">OnlineJobs.ph</span>
            </div>
          </div>
        </div>

        {/* Table area */}
        <div class="flex-1 flex flex-col min-h-0 min-w-0">

          {/* Fixed header */}
          <div ref={headerEl!} class="shrink-0 min-w-0 overflow-hidden px-3 sm:px-5 pt-3" style={{ background: "var(--mk-bg)" }}>
            <div class="flex items-center border-b border-mk-separator pb-1" style={{ width: `${totalWidth()}px` }}>
              <div style={{ width: `${leadColWidth()}px`, "min-width": `${leadColWidth()}px` }} />
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
                <col style={{ width: `${leadColWidth()}px` }} />
                <For each={widths()}>{(w) => <col style={{ width: `${w}px` }} />}</For>
              </colgroup>
              <tbody>
                <Show
                  when={!props.jobs.loading}
                  fallback={<tr><td colspan="8" class="text-center py-16 text-[13px] text-mk-tertiary">Loading...</td></tr>}
                >
                  <Show
                    when={totalCount() > 0}
                    fallback={<tr><td colspan="8" class="text-center py-16 text-[13px] text-mk-tertiary">No jobs yet</td></tr>}
                  >
                    <For each={visibleJobs()}>
                      {(group) => (
                        <>
                          {/* Group header — only shown when viewing All */}
                          <Show when={selectedKeyword() === null}>
                            <tr>
                              <td colspan="8" style={{ padding: "0" }}>
                                <div class="flex items-center gap-2 px-2 pt-4 pb-2" style={{ width: `${totalWidth()}px` }}>
                                  <span class="text-[11px] font-semibold uppercase tracking-widest text-mk-cyan">{group.keyword}</span>
                                  <span class="text-[11px] text-mk-tertiary">{group.jobs.length}</span>
                                  <div class="flex-1 h-px" style={{ background: "var(--mk-separator)" }} />
                                </div>
                              </td>
                            </tr>
                          </Show>
                          <For each={group.jobs}>
                            {(job) => (
                              <tr class="border-b border-mk-separator/50 hover:bg-mk-fill transition-colors">
                                <td class={`text-center py-2.5 ${selectedKeyword() === null ? "pl-3" : ""}`}>
                                  <button
                                    class={`text-[15px] leading-none transition-colors ${job.watchlisted ? "text-mk-yellow" : "text-mk-tertiary hover:text-mk-yellow"}`}
                                    onClick={() => props.onToggleWatchlist(job.id)}
                                  >{job.watchlisted ? "\u2605" : "\u2606"}</button>
                                </td>
                                <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[12px] text-mk-secondary">{formatDate(job.posted_at)}</span></td>
                                <td class="px-2 py-2.5 overflow-hidden">
                                  <span class="block truncate text-[13px] font-medium text-mk-text">
                                    {job.title}
                                    <Show when={job.is_new}><span class="ml-1.5 px-1 py-px rounded text-[9px] font-bold bg-mk-green-dim text-mk-green">NEW</span></Show>
                                  </span>
                                </td>
                                <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate"><span class="px-1.5 py-0.5 rounded text-[11px] bg-mk-fill text-mk-cyan border border-mk-separator">{job.keyword}</span></span></td>
                                <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[12px] text-mk-tertiary">{job.source}</span></td>
                                <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[13px] text-mk-secondary">{job.pay || "-"}</span></td>
                                <td class="px-2 py-2.5 overflow-hidden"><span class="block truncate text-[13px] text-mk-secondary">{job.company || "-"}</span></td>
                                <td class="text-center py-2.5">
                                  <button class="px-2 py-0.5 text-[11px] rounded-md text-mk-cyan hover:bg-mk-fill transition-all" onClick={() => openUrl(job.url)}>Open</button>
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
      </div>
    </div>
  );
}
