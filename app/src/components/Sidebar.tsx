import { For, Show, Resource, type Component } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CirclePlus, List, Moon, Star, Sun, Trash2 } from "lucide-solid";

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
  onRequestDeleteRun: (run: ScanRun) => void;
  onRequestClearAll: () => void;
}

const navItems: { id: View; label: string; Icon: Component<{ class?: string }> }[] = [
  { id: "jobs", label: "All Jobs", Icon: List },
  { id: "watchlist", label: "Watchlist", Icon: Star },
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
  const handleWindowDrag = (e: MouseEvent) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("button,input,a,textarea,select,[role='button']")) return;
    void getCurrentWindow().startDragging();
  };

  return (
    <aside class="w-52 bg-mk-sidebar flex flex-col shrink-0 h-screen border-r border-mk-sidebar-sep">
      {/* Titlebar drag region */}
      <div
        class="h-14 shrink-0"
        onMouseDown={handleWindowDrag}
      />

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
          <CirclePlus class="w-3.5 h-3.5" />
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
                    <item.Icon class="w-[15px] h-[15px] shrink-0 opacity-75" />
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
            onClick={props.onRequestClearAll}
            title="Clear all jobs and history"
            aria-label="Clear all jobs and scan history"
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
                <div class="group flex items-start justify-between gap-2 px-2.5 py-1.5 rounded-md hover:bg-mk-sidebar-hover transition-colors">
                  <div class="min-w-0">
                    <p class="text-[11px] text-mk-sidebar-secondary truncate">{formatRunDate(run.started_at)}</p>
                    <p class="text-[10px] text-mk-sidebar-tertiary truncate mt-0.5">{run.keywords}</p>
                    <p class="text-[10px] mt-0.5">
                      <span class="text-mk-green font-medium">+{run.total_new} new</span>
                      <span class="text-mk-sidebar-tertiary"> / {run.total_found} found</span>
                    </p>
                  </div>
                  <button
                    class="mt-0.5 shrink-0 text-mk-sidebar-tertiary hover:text-mk-pink transition-colors"
                    title="Delete this scan and its jobs"
                    aria-label="Delete scan run"
                    onClick={() => props.onRequestDeleteRun(run)}
                  >
                    <Trash2 class="w-3.5 h-3.5" />
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
          aria-label={props.dark ? "Switch to light theme" : "Switch to dark theme"}
        >
          <span class="absolute left-1.5 w-3.5 h-3.5 flex items-center justify-center text-white opacity-70">
            <Sun class="w-3.5 h-3.5" />
          </span>
          <span class="absolute right-1.5 w-3.5 h-3.5 flex items-center justify-center text-white opacity-70">
            <Moon class="w-3.5 h-3.5" />
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
