import { createSignal, For, Show, Accessor, Resource, Setter, onMount } from "solid-js";
import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { X } from "lucide-solid";
import toast from "solid-toast";
import AnimatedNumber from "../components/AnimatedNumber";
import { runMutation } from "../utils/mutations";
import { animateViewEnter } from "../utils/viewMotion";
import type { CrawlStats, ScanProgress } from "../types/ipc";

const PAGES_PER_KEYWORD = 5;

interface ProgressSnapshot {
  totalKeywords: number;
  keywordIndex: number;
  currentKeyword: string;
  currentPage: number;
  liveFound: number;
  liveNew: number;
}

interface ScanViewProps {
  crawling: Accessor<boolean>;
  setCrawling: (v: boolean) => void;
  crawlResult: Accessor<CrawlStats[] | null>;
  setCrawlResult: (v: CrawlStats[] | null) => void;
  crawlError: Accessor<string>;
  setCrawlError: (v: string) => void;
  keywords: Resource<string[]>;
  dateRange: Accessor<number>;
  setDateRange: Setter<number>;
  enabledSources: Accessor<string[]>;
  setEnabledSources: (v: string[]) => void;
  onScanStart: () => void;
  onScanComplete: () => void;
  onKeywordsChange: () => void;
}

export default function ScanView(props: ScanViewProps) {
  const [newKeyword, setNewKeyword] = createSignal("");
  const [progress, setProgress] = createSignal<ProgressSnapshot | null>(null);
  let viewEl!: HTMLDivElement;
  const handleWindowDrag = (e: MouseEvent) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("button,input,a,textarea,select,[role='button']")) return;
    void getCurrentWindow().startDragging();
  };

  // Map a ProgressSnapshot to a 0..1 fraction. Each keyword owns an equal slice
  // of the bar; within a keyword we advance proportionally to the page count.
  const progressFraction = (): number => {
    const p = progress();
    if (!p || p.totalKeywords === 0) return 0;
    const perKeyword = 1 / p.totalKeywords;
    const within = Math.min(p.currentPage, PAGES_PER_KEYWORD) / PAGES_PER_KEYWORD;
    return Math.min(1, perKeyword * (p.keywordIndex + within));
  };

  const handleCrawl = async () => {
    const loadingToast = toast.loading("Scanning jobs...");
    props.setCrawling(true);
    props.setCrawlResult(null);
    props.setCrawlError("");
    setProgress(null);
    props.onScanStart();

    const channel = new Channel<ScanProgress>();
    let liveFound = 0;
    let liveNew = 0;

    channel.onmessage = (msg) => {
      switch (msg.kind) {
        case "started":
          setProgress({
            totalKeywords: msg.total_keywords,
            keywordIndex: 0,
            currentKeyword: msg.keywords[0] ?? "",
            currentPage: 0,
            liveFound: 0,
            liveNew: 0,
          });
          break;
        case "keyword_started":
          setProgress((prev) => ({
            totalKeywords: prev?.totalKeywords ?? msg.total,
            keywordIndex: msg.index,
            currentKeyword: msg.keyword,
            currentPage: 0,
            liveFound,
            liveNew,
          }));
          break;
        case "page":
          setProgress((prev) =>
            prev
              ? { ...prev, currentKeyword: msg.keyword, currentPage: msg.page, liveFound: liveFound + msg.found }
              : prev
          );
          break;
        case "keyword_completed":
          liveFound += msg.found;
          liveNew += msg.new;
          setProgress((prev) =>
            prev ? { ...prev, currentPage: msg.pages, liveFound, liveNew } : prev
          );
          break;
        case "bruntwork_keyword":
          liveFound += msg.found;
          liveNew += msg.new;
          setProgress((prev) =>
            prev ? { ...prev, liveFound, liveNew } : prev
          );
          break;
        case "completed":
          setProgress((prev) =>
            prev
              ? {
                  ...prev,
                  keywordIndex: prev.totalKeywords,
                  currentPage: PAGES_PER_KEYWORD,
                  liveFound: Number(msg.total_found),
                  liveNew: Number(msg.total_new),
                }
              : prev
          );
          break;
        case "failed":
          // Error handling lands in the catch below; nothing extra to do here.
          break;
      }
    };

    try {
      const stats = await invoke<CrawlStats[]>("crawl_jobs", {
        days: props.dateRange(),
        sources: props.enabledSources(),
        onProgress: channel,
      });
      props.setCrawlResult(stats);
      props.onScanComplete();
      const found = stats.reduce((sum, s) => sum + s.found, 0);
      const fresh = stats.reduce((sum, s) => sum + s.new, 0);
      toast.success(`Scan complete: ${fresh} new, ${found} total.`, { id: loadingToast });
    } catch (e: any) {
      const message = String(e);
      props.setCrawlError(message);
      toast.error(`Scan failed: ${message}`, { id: loadingToast });
    }
    props.setCrawling(false);
    setProgress(null);
  };

  const handleAddKeyword = async () => {
    const kw = newKeyword().trim();
    if (!kw) return;
    const ok = await runMutation(
      () => invoke("add_keyword", { keyword: kw }),
      () => {
        setNewKeyword("");
        props.onKeywordsChange();
      },
      props.setCrawlError
    );
    if (ok) {
      toast.success(`Keyword added: ${kw}`);
    } else {
      toast.error("Failed to add keyword.");
    }
  };

  const handleRemoveKeyword = async (kw: string) => {
    const ok = await runMutation(
      () => invoke("remove_keyword", { keyword: kw }),
      props.onKeywordsChange,
      props.setCrawlError
    );
    if (ok) {
      toast.success(`Keyword removed: ${kw}`);
    } else {
      toast.error("Failed to remove keyword.");
    }
  };

  const totalNew = () => props.crawlResult()?.reduce((sum, s) => sum + s.new, 0) ?? 0;
  const totalFound = () => props.crawlResult()?.reduce((sum, s) => sum + s.found, 0) ?? 0;

  onMount(() => {
    animateViewEnter(viewEl);
  });

  return (
    <div ref={viewEl!} class="flex-1 flex flex-col bg-mk-bg">
      <div
        class="app-titlebar shrink-0"
        onMouseDown={handleWindowDrag}
      />

      <div class="flex-1 overflow-y-auto">
        <div class="max-w-xl mx-auto px-6 pb-12">
          {/* Heading */}
          <div class="app-title-card mb-6">
            <h2 class="text-[20px] font-semibold text-mk-text tracking-tight">New Scan</h2>
            <p class="text-[12px] text-mk-secondary mt-1">Add keywords and start crawling for jobs.</p>
          </div>

          {/* Keywords */}
          <div class="app-surface p-4 sm:p-4 mb-6">
            <label class="block text-[11px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2.5">Keywords</label>
            <div class="flex gap-2 mb-3">
              <input
                class="app-input flex-1 px-3 text-[13px] text-mk-text outline-none placeholder-mk-tertiary"
                type="text"
                placeholder="e.g. web developer"
                value={newKeyword()}
                onInput={(e) => setNewKeyword(e.currentTarget.value)}
                onKeyDown={(e) => e.key === "Enter" && handleAddKeyword()}
              />
              <button
                class="hover-lift px-4 text-[12px] font-semibold rounded-lg bg-mk-green hover:bg-mk-green-hover active:scale-[0.97] transition-all"
                style={{ height: "var(--ux-control-h)", color: "var(--mk-sidebar)" }}
                onClick={handleAddKeyword}
              >
                Add
              </button>
            </div>
            <div class="flex flex-wrap gap-1.5">
              <Show when={props.keywords()}>
                <For each={props.keywords()!} fallback={<p class="text-[13px] text-mk-tertiary italic">No keywords yet</p>}>
                  {(kw) => (
                    <span class="inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-[12px] bg-mk-fill text-mk-secondary border border-mk-separator">
                      {kw}
                      <button
                        class="text-mk-tertiary hover:text-mk-pink transition-colors ml-0.5"
                        aria-label={`Remove keyword ${kw}`}
                        onClick={() => handleRemoveKeyword(kw)}
                      >
                        <X class="w-3 h-3" />
                      </button>
                    </span>
                  )}
                </For>
              </Show>
            </div>
          </div>

          {/* Date range */}
          <div class="app-surface p-4 sm:p-4 mb-6">
            <label class="block text-[11px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2.5">Posted Within</label>
            <div class="flex gap-1.5">
              {([
                { label: "Today", days: 1 },
                { label: "3 days", days: 3 },
                { label: "1 week", days: 7 },
                { label: "2 weeks", days: 14 },
              ] as const).map(({ label, days }) => (
                <button
                  class={`flex-1 py-1.5 text-[11px] font-medium rounded-lg border transition-all ${
                    props.dateRange() === days
                      ? "bg-mk-green border-mk-green"
                      : "bg-mk-fill border-mk-separator text-mk-secondary hover:border-mk-green/50 hover:text-mk-text"
                  }`}
                  style={props.dateRange() === days ? { color: "var(--mk-sidebar)" } : {}}
                  onClick={() => props.setDateRange(days)}
                >
                  {label}
                </button>
              ))}
            </div>
            <p class="text-[11px] text-mk-tertiary mt-2">
              Only jobs posted in the last {props.dateRange() === 1 ? "24 hours" : `${props.dateRange()} days`} will be fetched and shown.
            </p>
          </div>

          {/* Sources */}
          <div class="app-surface p-4 sm:p-4 mb-6">
            <label class="block text-[11px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2.5">Sources</label>
            <div class="flex flex-col gap-2.5">
              {([
                { id: "onlinejobs", label: "OnlineJobs.ph", dot: "bg-mk-green" },
                { id: "bruntwork", label: "BruntWork Careers", dot: "bg-mk-cyan" },
              ] as const).map(({ id, label, dot }) => {
                const checked = () => props.enabledSources().includes(id);
                const toggle = () => {
                  if (checked()) {
                    props.setEnabledSources(props.enabledSources().filter((s) => s !== id));
                  } else {
                    props.setEnabledSources([...props.enabledSources(), id]);
                  }
                };
                return (
                  <button
                    type="button"
                    onClick={toggle}
                    class={`flex items-center gap-3 px-3 py-2 rounded-lg border transition-all text-left ${
                      checked()
                        ? "border-mk-separator bg-mk-fill"
                        : "border-mk-separator/50 bg-transparent opacity-50"
                    }`}
                  >
                    <span class={`w-4 h-4 rounded flex items-center justify-center border shrink-0 transition-colors ${
                      checked() ? "bg-mk-green border-mk-green" : "border-mk-separator bg-mk-fill"
                    }`}>
                      <Show when={checked()}>
                        <svg class="w-2.5 h-2.5" viewBox="0 0 10 8" fill="none">
                          <path d="M1 4l3 3 5-6" stroke="var(--mk-sidebar)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                        </svg>
                      </Show>
                    </span>
                    <span class={`w-1.5 h-1.5 rounded-full shrink-0 ${dot}`} />
                    <span class="text-[13px] text-mk-secondary">{label}</span>
                  </button>
                );
              })}
            </div>
          </div>

          {/* Scan button */}
          <div class="mb-6">
            <button
              class={`hover-lift w-full py-2.5 text-[13px] font-semibold rounded-xl transition-all ${
                props.crawling()
                  ? "opacity-40 cursor-not-allowed bg-mk-green"
                  : "bg-mk-green hover:bg-mk-green-hover active:scale-[0.99] shadow-sm"
              }`}
              style={{ color: "var(--mk-sidebar)" }}
              onClick={handleCrawl}
              disabled={props.crawling()}
            >
              {props.crawling() ? "Scanning..." : "Scan Now"}
            </button>
          </div>

          {/* Scanning state */}
          <Show when={props.crawling()}>
            <div class="app-surface p-5 mb-4">
              <div class="flex items-center gap-3">
                <div class="relative h-4 w-4 shrink-0">
                  <div class="absolute inset-0 rounded-full border-[1.5px] border-mk-separator" />
                  <div class="absolute inset-0 rounded-full border-[1.5px] border-mk-cyan border-t-transparent animate-spin" />
                </div>
                <div class="min-w-0 flex-1">
                  <p class="text-[13px] font-medium text-mk-text">
                    <Show when={progress()} fallback="Starting scan…">
                      {(p) => (
                        <>
                          Scanning <span class="text-mk-cyan">{p().currentKeyword || "…"}</span>
                          <Show when={p().currentPage > 0}>
                            <span class="text-mk-tertiary">
                              {" "}— page {p().currentPage}/{PAGES_PER_KEYWORD}
                            </span>
                          </Show>
                        </>
                      )}
                    </Show>
                  </p>
                  <p class="text-[11px] text-mk-tertiary mt-0.5">
                    <Show when={progress()} fallback="Connecting to sources…">
                      {(p) => (
                        <>
                          Keyword {Math.min(p().keywordIndex + 1, p().totalKeywords)}/{p().totalKeywords}
                          <span class="mx-1">·</span>
                          {p().liveFound} found
                          <span class="mx-1">·</span>
                          {p().liveNew} new
                        </>
                      )}
                    </Show>
                  </p>
                </div>
              </div>
              <div class="mt-3 h-[3px] bg-mk-fill rounded-full overflow-hidden">
                <div
                  class="h-full bg-mk-cyan rounded-full transition-[width] duration-300 ease-out"
                  style={{ width: `${Math.max(4, progressFraction() * 100)}%` }}
                />
              </div>
            </div>
          </Show>

          {/* Results */}
          <Show when={props.crawlResult() && !props.crawling()}>
            <div class="rounded-xl bg-mk-grouped-bg border border-mk-green-dim p-5 mb-4">
              <p class="text-[13px] font-medium text-mk-green mb-2">
                Done — <AnimatedNumber value={totalNew()} class="inline-block" /> new, <AnimatedNumber value={totalFound()} class="inline-block" /> total
              </p>
              <div class="flex flex-wrap gap-1.5">
                <For each={props.crawlResult()!}>
                  {(s) => (
                    <span class="px-2 py-0.5 rounded-full text-[11px] font-medium bg-mk-green-dim text-mk-green">
                      {s.keyword} +{s.new}
                    </span>
                  )}
                </For>
              </div>
            </div>
          </Show>

          {/* Error */}
          <Show when={props.crawlError()}>
            <div class="app-surface border-mk-pink/20 p-5">
              <p class="text-[13px] text-mk-pink">{props.crawlError()}</p>
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
}
