import { createSignal, For, Show, Accessor, Resource, Setter } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { X } from "lucide-solid";
import toast from "solid-toast";
import AnimatedNumber from "../components/AnimatedNumber";
import { runMutation } from "../utils/mutations";

interface CrawlStats {
  keyword: string;
  found: number;
  new: number;
  pages: number;
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
  onScanStart: () => void;
  onScanComplete: () => void;
}

export default function ScanView(props: ScanViewProps) {
  const [newKeyword, setNewKeyword] = createSignal("");
  const handleWindowDrag = (e: MouseEvent) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("button,input,a,textarea,select,[role='button']")) return;
    void getCurrentWindow().startDragging();
  };

  const handleCrawl = async () => {
    const loadingToast = toast.loading("Scanning jobs...");
    props.setCrawling(true);
    props.setCrawlResult(null);
    props.setCrawlError("");
    props.onScanStart();
    try {
      const stats = await invoke<CrawlStats[]>("crawl_jobs", { days: props.dateRange() });
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
  };

  const handleAddKeyword = async () => {
    const kw = newKeyword().trim();
    if (!kw) return;
    const ok = await runMutation(
      () => invoke("add_keyword", { keyword: kw }),
      () => {
        setNewKeyword("");
        props.onScanComplete();
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
      props.onScanComplete,
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

  return (
    <div class="flex-1 flex flex-col bg-mk-bg">
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
                <div>
                  <p class="text-[13px] font-medium text-mk-text">Scanning OnlineJobs.ph</p>
                  <p class="text-[12px] text-mk-tertiary mt-0.5">This may take a moment.</p>
                </div>
              </div>
              <div class="mt-3 h-[3px] bg-mk-fill rounded-full overflow-hidden">
                <div class="h-full bg-mk-cyan rounded-full animate-pulse" style="width: 55%" />
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
