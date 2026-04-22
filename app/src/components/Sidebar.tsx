import { For, Show, Resource, type Component } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Bot, CirclePlus, List, Settings2, Star, Trash2 } from "lucide-solid";
import type { ScanRun } from "../types/ipc";

export type View = "scan" | "jobs" | "watchlist" | "ezer";

interface SidebarProps {
  currentView: View;
  onNavigate: (view: View) => void;
  crawling: boolean;
  dark: boolean;
  onToggleTheme: () => void;
  onOpenSettings: () => void;
  runs: Resource<ScanRun[]>;
  onRequestDeleteRun: (run: ScanRun) => void;
  onRequestClearAll: () => void;
}

const navItems: { id: View; label: string; Icon: Component<{ class?: string }> }[] = [
  { id: "jobs", label: "All Jobs", Icon: List },
  { id: "watchlist", label: "Watchlist", Icon: Star },
  { id: "ezer", label: "Ezer AI", Icon: Bot },
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
      <div class="px-3 mb-4">
        <button
          class={`hover-lift w-full flex items-center justify-center gap-2 py-2 rounded-lg text-[13px] font-semibold transition-all active:scale-[0.97] ${
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
                  class={`sidebar-nav-item w-full text-left px-2.5 py-[5px] text-[12px] transition-all ${
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
      <div class="px-2.5 mt-5 flex-1 flex flex-col min-h-0">
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
        <div class="flex-1 overflow-y-auto space-y-0.5">
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

      {/* Footer controls */}
      <div class="px-4 py-3 border-t border-mk-sidebar-sep flex items-center justify-start gap-2 shrink-0">
        <button
          class="inline-flex items-center gap-1.5 px-2 py-1.5 rounded-lg text-[11px] font-medium text-mk-sidebar-secondary hover:text-mk-sidebar-txt hover:bg-mk-sidebar-hover transition-colors"
          onClick={props.onOpenSettings}
          title="Open preferences"
          aria-label="Open preferences"
        >
          <Settings2 class="w-4 h-4" />
          <span>Preferences</span>
        </button>
      </div>
    </aside>
  );
}
