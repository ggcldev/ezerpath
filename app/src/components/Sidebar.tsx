import { For, Show, Resource } from "solid-js";

export type View = "scan" | "jobs" | "watchlist";

export interface ScanRun {
  id: number;
  started_at: string;
  keywords: string;
  total_found: number;
  total_new: number;
}

interface SidebarProps {
  currentView: View;
  onNavigate: (view: View) => void;
  crawling: boolean;
  dark: boolean;
  onToggleTheme: () => void;
  runs: Resource<ScanRun[]>;
  onDeleteRun: (runId: number) => void;
  onClearAll: () => void;
}

const navItems: { id: View; label: string; icon: string }[] = [
  { id: "jobs", label: "All Jobs", icon: "M3.75 12h16.5m-16.5 3.75h16.5M3.75 19.5h16.5M5.625 4.5h12.75a1.875 1.875 0 010 3.75H5.625a1.875 1.875 0 010-3.75z" },
  { id: "watchlist", label: "Watchlist", icon: "M11.48 3.499a.562.562 0 011.04 0l2.125 5.111a.563.563 0 00.475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 00-.182.557l1.285 5.385a.562.562 0 01-.84.61l-4.725-2.885a.563.563 0 00-.586 0L6.982 20.54a.562.562 0 01-.84-.61l1.285-5.386a.562.562 0 00-.182-.557l-4.204-3.602a.563.563 0 01.321-.988l5.518-.442a.563.563 0 00.475-.345L11.48 3.5z" },
];

function formatRunDate(raw: string): string {
  if (!raw) return "";
  const d = new Date(raw);
  if (isNaN(d.getTime())) return raw;
  const month = d.toLocaleString("en-US", { month: "short" });
  const day = d.getDate();
  const time = d.toLocaleString("en-US", { hour: "numeric", minute: "2-digit", hour12: true });
  return `${month} ${day} · ${time}`;
}

export default function Sidebar(props: SidebarProps) {
  return (
    <aside class="w-52 bg-mk-sidebar flex flex-col shrink-0 h-screen border-r border-mk-sidebar-sep">
      {/* Titlebar drag region */}
      <div class="h-12 shrink-0" data-tauri-drag-region />

      {/* New Scan button */}
      <div class="px-3 mb-5">
        <button
          class={`w-full flex items-center justify-center gap-2 py-2 rounded-lg text-[13px] font-semibold transition-all active:scale-[0.97] ${
            props.crawling
              ? "opacity-40 cursor-not-allowed bg-mk-green text-mk-sidebar"
              : "bg-mk-green text-mk-sidebar hover:bg-mk-green-hover shadow-sm"
          }`}
          style={{ color: "var(--mk-sidebar)" }}
          onClick={() => props.onNavigate("scan")}
          disabled={props.crawling}
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke-width="2.5" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
          </svg>
          New Scan
          <Show when={props.crawling}>
            <span class="ml-1 w-[5px] h-[5px] rounded-full bg-current opacity-60 animate-pulse" />
          </Show>
        </button>
      </div>

      {/* Navigation */}
      <nav class="px-2.5">
        <ul class="space-y-0.5">
          <For each={navItems}>
            {(item) => (
              <li>
                <button
                  class={`w-full text-left px-2.5 py-[5px] rounded-md text-[13px] transition-all ${
                    props.currentView === item.id
                      ? "bg-mk-sidebar-active-bg text-mk-sidebar-active-txt font-medium"
                      : "text-mk-sidebar-secondary hover:bg-mk-sidebar-hover hover:text-mk-sidebar-txt"
                  }`}
                  onClick={() => props.onNavigate(item.id)}
                >
                  <span class="flex items-center gap-2.5">
                    <svg class="w-[15px] h-[15px] shrink-0 opacity-75" fill="none" viewBox="0 0 24 24" stroke-width="1.8" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" d={item.icon} />
                    </svg>
                    {item.label}
                  </span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </nav>

      {/* Scan History */}
      <div class="px-2.5 mt-6 flex-1 flex flex-col min-h-0">
        <div class="flex items-center justify-between px-2.5 mb-1.5">
          <p class="text-[10px] font-semibold uppercase tracking-widest text-mk-sidebar-tertiary">Scan History</p>
          <button
            class="text-[10px] text-mk-sidebar-tertiary hover:text-mk-pink transition-colors"
            onClick={props.onClearAll}
            title="Clear all jobs and history"
          >
            Clear all
          </button>
        </div>
        <div class="flex-1 overflow-y-auto space-y-px">
          <Show
            when={(props.runs() ?? []).length > 0}
            fallback={
              <p class="px-2.5 text-[11px] text-mk-sidebar-tertiary italic">No scans yet</p>
            }
          >
            <For each={props.runs() ?? []}>
              {(run) => (
                <div class="group flex items-start justify-between px-2.5 py-1.5 rounded-md hover:bg-mk-sidebar-hover transition-colors">
                  <div class="min-w-0">
                    <p class="text-[11px] text-mk-sidebar-secondary truncate">{formatRunDate(run.started_at)}</p>
                    <p class="text-[10px] text-mk-sidebar-tertiary truncate mt-0.5">{run.keywords}</p>
                    <p class="text-[10px] mt-0.5">
                      <span class="text-mk-green font-medium">+{run.total_new} new</span>
                      <span class="text-mk-sidebar-tertiary"> / {run.total_found} found</span>
                    </p>
                  </div>
                  <button
                    class="ml-1.5 mt-0.5 shrink-0 opacity-0 group-hover:opacity-100 text-mk-sidebar-tertiary hover:text-mk-pink transition-all"
                    title="Delete run"
                    onClick={() => props.onDeleteRun(run.id)}
                  >
                    <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>
              )}
            </For>
          </Show>
        </div>
      </div>

      {/* Theme toggle */}
      <div class="px-4 py-3 border-t border-mk-sidebar-sep flex items-center justify-center shrink-0">
        <button
          class="relative flex items-center w-12 h-6 rounded-full transition-colors duration-200 focus:outline-none"
          style={{ background: props.dark ? "var(--mk-green)" : "rgba(0,0,0,0.28)" }}
          onClick={props.onToggleTheme}
          title={props.dark ? "Switch to light" : "Switch to dark"}
        >
          <span class="absolute left-1.5 w-3.5 h-3.5 flex items-center justify-center text-white opacity-70">
            <svg fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 3v2.25m6.364.386l-1.591 1.591M21 12h-2.25m-.386 6.364l-1.591-1.591M12 18.75V21m-4.773-4.227l-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0z" />
            </svg>
          </span>
          <span class="absolute right-1.5 w-3.5 h-3.5 flex items-center justify-center text-white opacity-70">
            <svg fill="none" viewBox="0 0 24 24" stroke-width="2" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" d="M21.752 15.002A9.718 9.718 0 0118 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 003 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 009.002-5.998z" />
            </svg>
          </span>
          <span
            class="absolute w-5 h-5 bg-white rounded-full shadow-sm transition-transform duration-200"
            style={{ transform: props.dark ? "translateX(24px)" : "translateX(2px)" }}
          />
        </button>
      </div>
    </aside>
  );
}
