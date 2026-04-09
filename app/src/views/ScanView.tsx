import { createSignal, For, Show, Accessor, Resource, Setter } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

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

  const handleCrawl = async () => {
    props.setCrawling(true);
    props.setCrawlResult(null);
    props.setCrawlError("");
    props.onScanStart();
    try {
      const stats = await invoke<CrawlStats[]>("crawl_jobs", { days: props.dateRange() });
      props.setCrawlResult(stats);
      props.onScanComplete();
    } catch (e: any) {
      props.setCrawlError(String(e));
    }
    props.setCrawling(false);
  };

  const handleAddKeyword = async () => {
    const kw = newKeyword().trim();
    if (!kw) return;
    await invoke("add_keyword", { keyword: kw });
    setNewKeyword("");
    props.onScanComplete();
  };

  const handleRemoveKeyword = async (kw: string) => {
    await invoke("remove_keyword", { keyword: kw });
    props.onScanComplete();
  };

  const totalNew = () => props.crawlResult()?.reduce((sum, s) => sum + s.new, 0) ?? 0;
  const totalFound = () => props.crawlResult()?.reduce((sum, s) => sum + s.found, 0) ?? 0;

  return (
    <div class="flex-1 flex flex-col bg-mk-bg">
      <div class="h-12 shrink-0" data-tauri-drag-region />

      <div class="flex-1 overflow-y-auto">
        <div class="max-w-xl mx-auto px-6 pb-12">
          {/* Heading */}
          <div class="mb-8">
            <h2 class="text-[22px] font-semibold text-mk-text tracking-tight">New Scan</h2>
            <p class="text-[13px] text-mk-secondary mt-1">Add keywords and start crawling for jobs.</p>
          </div>

          {/* Keywords */}
          <div class="mb-8">
            <label class="block text-[11px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2.5">Keywords</label>
            <div class="flex gap-2 mb-3">
              <input
                class="flex-1 px-3 py-2 text-[13px] rounded-lg bg-mk-grouped-bg border border-mk-separator text-mk-text outline-none focus:border-mk-green focus:ring-2 focus:ring-mk-green-dim placeholder-mk-tertiary transition-all"
                type="text"
                placeholder="e.g. web developer"
                value={newKeyword()}
                onInput={(e) => setNewKeyword(e.currentTarget.value)}
                onKeyDown={(e) => e.key === "Enter" && handleAddKeyword()}
              />
              <button
                class="px-4 py-2 text-[13px] font-semibold rounded-lg bg-mk-green hover:bg-mk-green-hover active:scale-[0.97] transition-all"
                style={{ color: "var(--mk-sidebar)" }}
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
                        onClick={() => handleRemoveKeyword(kw)}
                      >
                        <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
                          <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                        </svg>
                      </button>
                    </span>
                  )}
                </For>
              </Show>
            </div>
          </div>

          {/* Date range */}
          <div class="mb-8">
            <label class="block text-[11px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2.5">Posted Within</label>
            <div class="flex gap-1.5">
              {([
                { label: "Today", days: 1 },
                { label: "3 days", days: 3 },
                { label: "1 week", days: 7 },
                { label: "2 weeks", days: 14 },
              ] as const).map(({ label, days }) => (
                <button
                  class={`flex-1 py-1.5 text-[12px] font-medium rounded-lg border transition-all ${
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
          <div class="mb-8">
            <button
              class={`w-full py-3 text-[14px] font-semibold rounded-xl transition-all ${
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
            <div class="rounded-xl bg-mk-grouped-bg border border-mk-separator p-5 mb-4">
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
                Done — {totalNew()} new, {totalFound()} total
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
            <div class="rounded-xl bg-mk-grouped-bg border border-mk-pink/20 p-5">
              <p class="text-[13px] text-mk-pink">{props.crawlError()}</p>
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
}
