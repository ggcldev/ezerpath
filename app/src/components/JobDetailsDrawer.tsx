import { Show, createEffect, onCleanup } from "solid-js";
import { Building2, CalendarDays, Tag, Wallet, X } from "lucide-solid";

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
        <aside class="absolute top-0 right-0 bottom-0 z-30 w-[390px] max-w-[45vw] min-w-[320px] border-l border-mk-separator bg-mk-grouped-bg/92 backdrop-blur-md flex flex-col min-h-0 shadow-2xl animate-drawer-in">
          <div class="px-4 py-3 border-b border-mk-separator/80 bg-gradient-to-b from-mk-fill/45 to-transparent flex items-start justify-between gap-3">
            <div class="min-w-0">
              <p class="text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Job Details</p>
              <h3 class="mt-1 text-[14px] font-semibold text-mk-text leading-snug break-words">{job().title}</h3>
            </div>
            <button
              class="shrink-0 p-1 rounded-md text-mk-tertiary hover:text-mk-text hover:bg-mk-fill transition-all"
              aria-label="Close details panel"
              onClick={props.onClose}
            >
              <X class="w-4 h-4" />
            </button>
          </div>

          <div class="flex-1 overflow-auto p-4 space-y-4">
            <div class="rounded-xl border border-mk-separator bg-mk-fill/35 p-3">
              <div class="flex items-center gap-3">
              <Show
                when={job().company_logo_url && /^https?:\/\//.test(job().company_logo_url)}
                fallback={
                  <div class="w-12 h-12 rounded-md bg-mk-fill border border-mk-separator flex items-center justify-center text-mk-secondary">
                    <Building2 class="w-5 h-5" />
                  </div>
                }
              >
                <img
                  src={job().company_logo_url}
                  alt={job().company || "Company logo"}
                  class="w-12 h-12 rounded-md object-cover border border-mk-separator bg-mk-fill"
                  referrerPolicy="no-referrer"
                />
              </Show>
              <div class="min-w-0">
                <p class="text-[13px] font-semibold text-mk-text truncate">{job().company || "Unknown company"}</p>
                <p class="text-[11px] text-mk-tertiary">{job().source}</p>
              </div>
            </div>
            </div>

            <div class="grid grid-cols-2 gap-2 text-[11px]">
              <div class="rounded-md border border-mk-separator bg-mk-fill/60 px-2.5 py-2">
                <p class="text-mk-tertiary flex items-center gap-1"><CalendarDays class="w-3.5 h-3.5" /> Posted</p>
                <p class="text-mk-secondary mt-0.5">{formatPosted(job().posted_at)}</p>
              </div>
              <div class="rounded-md border border-mk-separator bg-mk-fill/60 px-2.5 py-2">
                <p class="text-mk-tertiary flex items-center gap-1"><Wallet class="w-3.5 h-3.5" /> Pay</p>
                <p class="text-mk-secondary mt-0.5">{job().pay || "-"}</p>
              </div>
              <div class="rounded-md border border-mk-separator bg-mk-fill/60 px-2.5 py-2 col-span-2">
                <p class="text-mk-tertiary flex items-center gap-1"><Tag class="w-3.5 h-3.5" /> Keyword</p>
                <p class="text-mk-secondary mt-0.5">{job().keyword || "-"}</p>
              </div>
            </div>

            <div>
              <p class="text-[11px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2">Description</p>
              <p class="rounded-lg border border-mk-separator bg-mk-fill/35 px-3 py-2.5 text-[12px] leading-relaxed text-mk-secondary whitespace-pre-wrap break-words">
                {job().summary || "No description available from the listing preview."}
              </p>
            </div>
          </div>

          <div class="px-4 py-3 border-t border-mk-separator">
            <button
              class="w-full py-2 text-[12px] font-semibold rounded-lg bg-mk-green hover:bg-mk-green-hover transition-all"
              style={{ color: "var(--mk-sidebar)" }}
              onClick={() => props.onOpenUrl(job().url)}
            >
              Open On Platform
            </button>
          </div>
        </aside>
      )}
    </Show>
  );
}
