import { For, Show } from "solid-js";
import { Moon, Sun, X } from "lucide-solid";

interface SettingsPanelProps {
  open: boolean;
  dark: boolean;
  onToggleTheme: () => void;
  onClose: () => void;
}

const sections = [
  {
    title: "General",
    items: [
      "Theme & appearance",
      "Language & date format",
      "Default view on app launch",
    ],
  },
  {
    title: "Scanning",
    items: [
      "Keyword presets",
      "Default date range",
      "Background scan cadence",
    ],
  },
  {
    title: "Notifications",
    items: [
      "Desktop alerts",
      "Only notify new jobs",
      "Quiet hours",
    ],
  },
  {
    title: "AI Copilot",
    items: [
      "Provider and model settings",
      "Resume profile preferences",
      "Summary and ranking behavior",
    ],
  },
];

export default function SettingsPanel(props: SettingsPanelProps) {
  return (
    <Show when={props.open}>
      <div class="fixed inset-0 z-50 flex items-center justify-center">
        <button
          class="absolute inset-0 bg-black/30 backdrop-blur-[2px]"
          aria-label="Close settings panel"
          onClick={props.onClose}
        />
        <section class="relative w-[640px] max-w-[92vw] max-h-[85vh] overflow-auto rounded-xl border border-mk-separator bg-mk-grouped-bg shadow-2xl">
          <div class="sticky top-0 z-10 flex items-center justify-between px-5 py-4 border-b border-mk-separator bg-mk-grouped-bg/95 backdrop-blur-sm">
            <div>
              <p class="text-[11px] uppercase tracking-widest font-semibold text-mk-tertiary">Preferences</p>
              <h2 class="text-[18px] font-semibold text-mk-text mt-0.5">Settings</h2>
            </div>
            <button
              class="inline-flex items-center justify-center w-8 h-8 rounded-md text-mk-tertiary hover:text-mk-text hover:bg-mk-fill transition-colors"
              onClick={props.onClose}
              aria-label="Close settings"
            >
              <X class="w-4 h-4" />
            </button>
          </div>

          <div class="p-5 space-y-4">
            <div class="rounded-lg border border-mk-separator/80 bg-mk-bg/40 p-4">
              <h3 class="text-[14px] font-semibold text-mk-text">Appearance</h3>
              <div class="mt-2 flex items-center justify-between">
                <div>
                  <p class="text-[13px] text-mk-secondary">Theme</p>
                  <p class="text-[12px] text-mk-tertiary">{props.dark ? "Dark mode" : "Light mode"}</p>
                </div>
                <button
                  class="relative flex items-center w-14 h-7 rounded-full transition-colors duration-200 focus:outline-none"
                  style={{ background: props.dark ? "var(--mk-green)" : "rgba(0,0,0,0.28)" }}
                  onClick={props.onToggleTheme}
                  title={props.dark ? "Switch to light" : "Switch to dark"}
                  aria-label={props.dark ? "Switch to light theme" : "Switch to dark theme"}
                >
                  <span class="absolute left-2 w-3.5 h-3.5 flex items-center justify-center text-white opacity-70">
                    <Sun class="w-3.5 h-3.5" />
                  </span>
                  <span class="absolute right-2 w-3.5 h-3.5 flex items-center justify-center text-white opacity-70">
                    <Moon class="w-3.5 h-3.5" />
                  </span>
                  <span
                    class="absolute w-5 h-5 bg-white rounded-full shadow-sm transition-transform duration-200"
                    style={{ transform: props.dark ? "translateX(32px)" : "translateX(4px)" }}
                  />
                </button>
              </div>
            </div>

            <For each={sections}>
              {(section) => (
                <div class="rounded-lg border border-mk-separator/80 bg-mk-bg/40 p-4">
                  <h3 class="text-[14px] font-semibold text-mk-text">{section.title}</h3>
                  <ul class="mt-2 space-y-1">
                    <For each={section.items}>
                      {(item) => <li class="text-[13px] text-mk-secondary">{item}</li>}
                    </For>
                  </ul>
                </div>
              )}
            </For>
            <p class="text-[12px] text-mk-tertiary">
              This panel is now the central home for all app configuration and upcoming advanced options.
            </p>
          </div>
        </section>
      </div>
    </Show>
  );
}
