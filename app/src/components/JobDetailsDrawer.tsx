import { Show, createEffect, onCleanup } from "solid-js";
import { Building2, CalendarDays, ExternalLink, Tag, Wallet, X } from "lucide-solid";

interface JobDetails {
  title: string;
  company: string;
  company_logo_url?: string;
  source: string;
  pay: string;
  posted_at: string;
  keyword: string;
  summary: string;
  url: string;
}

interface JobDetailsDrawerProps {
  job: JobDetails | null;
  onClose: () => void;
  onOpenUrl: (url: string) => void;
}

function formatPosted(raw: string): string {
  if (!raw) return "-";
  const d = new Date(raw);
  if (isNaN(d.getTime())) return raw;
  const m = String(d.getMonth() + 1).padStart(2, "0");
  return `${m}/${d.getDate()}/${String(d.getFullYear()).slice(2)}`;
}

export default function JobDetailsDrawer(props: JobDetailsDrawerProps) {
  createEffect(() => {
    if (!props.job) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") props.onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    onCleanup(() => document.removeEventListener("keydown", onKeyDown));
  });

  return (
    <Show when={props.job}>
      {(job) => (
        <aside class="absolute top-0 right-0 bottom-0 z-30 w-[480px] max-w-[52vw] min-w-[360px] border-l border-mk-separator bg-mk-bg/96 backdrop-blur-sm flex flex-col min-h-0 shadow-2xl animate-drawer-in">
          <div class="sticky top-0 z-10 px-5 py-3 border-b border-mk-separator/80 bg-mk-bg/94 backdrop-blur-sm">
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Job Snapshot</p>
                <h3 class="mt-1 text-[15px] font-semibold text-mk-text leading-snug break-words">{job().title}</h3>
              </div>
              <button
                class="shrink-0 p-1 rounded-md text-mk-tertiary hover:text-mk-text hover:bg-mk-fill transition-all"
                aria-label="Close details panel"
                onClick={props.onClose}
              >
                <X class="w-4 h-4" />
              </button>
            </div>
            <div class="mt-2.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px]">
              <span class="inline-flex items-center gap-1 text-mk-secondary">
                <CalendarDays class="w-3.5 h-3.5 text-mk-tertiary" />
                {formatPosted(job().posted_at)}
              </span>
              <span class="inline-flex items-center gap-1 text-mk-secondary">
                <Wallet class="w-3.5 h-3.5 text-mk-tertiary" />
                {job().pay || "-"}
              </span>
              <span class="inline-flex items-center gap-1 text-mk-secondary">
                <Tag class="w-3.5 h-3.5 text-mk-tertiary" />
                {job().keyword || "-"}
              </span>
            </div>
          </div>

          <div class="flex-1 overflow-auto p-5 space-y-4">
            <div class="flex items-center gap-3 pb-3 border-b border-mk-separator/70">
              <Show
                when={job().company_logo_url && /^https?:\/\//.test(job().company_logo_url)}
                fallback={
                  <div class="w-12 h-12 rounded-md bg-mk-fill border border-mk-separator/80 flex items-center justify-center text-mk-secondary">
                    <Building2 class="w-5 h-5" />
                  </div>
                }
              >
                <img
                  src={job().company_logo_url}
                  alt={job().company || "Company logo"}
                  class="w-12 h-12 rounded-md object-cover border border-mk-separator/80 bg-mk-fill"
                  referrerPolicy="no-referrer"
                />
              </Show>
              <div class="min-w-0">
                <p class="text-[13px] font-semibold text-mk-text truncate">{job().company || "Unknown company"}</p>
                <p class="text-[11px] text-mk-tertiary">{job().source}</p>
              </div>
            </div>

            <div>
              <p class="text-[11px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2">Description</p>
              <p class="text-[12.5px] leading-relaxed text-mk-secondary whitespace-pre-wrap break-words">
                {job().summary || "No description available from the listing preview."}
              </p>
            </div>
          </div>

          <div class="px-5 py-3 border-t border-mk-separator flex items-center justify-between">
            <button
              class="text-[12px] font-medium text-mk-tertiary hover:text-mk-text transition-colors"
              onClick={props.onClose}
            >
              Close
            </button>
            <button
              class="inline-flex items-center gap-1 text-[12px] font-semibold text-mk-cyan hover:opacity-80 transition-opacity"
              onClick={() => props.onOpenUrl(job().url)}
            >
              Open full listing
              <ExternalLink class="w-3.5 h-3.5" />
            </button>
          </div>
        </aside>
      )}
    </Show>
  );
}
