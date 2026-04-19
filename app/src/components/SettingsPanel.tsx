import { For, Show } from "solid-js";
import { Moon, Sun, X } from "lucide-solid";

interface AiRuntimeConfig {
  ollama_base_url: string;
  ollama_model: string;
  embedding_service_url: string;
  embedding_model: string;
  temperature: number;
  max_tokens: number;
  timeout_ms: number;
}

interface ResumeProfile {
  id: number;
  name: string;
  source_file: string;
  is_active: boolean;
}

interface SettingsPanelProps {
  open: boolean;
  dark: boolean;
  onToggleTheme: () => void;
  aiConfig: AiRuntimeConfig;
  ollamaModels: string[];
  aiBusy: boolean;
  ollamaStatus: string;
  embeddingStatus: string;
  indexStatus: string;
  resumes: ResumeProfile[];
  selectedResumeId: number | null;
  resumeFilePath: string;
  resumeStatus: string;
  onAiConfigChange: (next: AiRuntimeConfig) => void;
  onSaveAiConfig: () => void;
  onRefreshOllamaModels: () => void;
  onCheckOllama: () => void;
  onCheckEmbedding: () => void;
  onIndexJobs: () => void;
  onResumeFilePathChange: (value: string) => void;
  onBrowseResumeFile: () => void;
  onUploadResumeFromPath: () => void;
  onSelectResume: (resumeId: number) => void;
  onIndexResume: () => void;
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

            <div class="rounded-lg border border-mk-separator/80 bg-mk-bg/40 p-4">
              <h3 class="text-[14px] font-semibold text-mk-text">AI Runtime</h3>
              <div class="mt-3 grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                <label class="text-[12px] text-mk-secondary">
                  Ollama URL
                  <input
                    class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                    value={props.aiConfig.ollama_base_url}
                    onInput={(e) => props.onAiConfigChange({ ...props.aiConfig, ollama_base_url: e.currentTarget.value })}
                  />
                </label>
                <label class="text-[12px] text-mk-secondary">
                  Ollama Model
                  <select
                    class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                    value={props.aiConfig.ollama_model}
                    onChange={(e) => props.onAiConfigChange({ ...props.aiConfig, ollama_model: e.currentTarget.value })}
                    disabled={props.ollamaModels.length === 0}
                  >
                    <Show
                      when={props.ollamaModels.length > 0}
                      fallback={<option value="">No models found</option>}
                    >
                      <For each={props.ollamaModels}>
                        {(model) => <option value={model}>{model}</option>}
                      </For>
                    </Show>
                  </select>
                </label>
                <label class="text-[12px] text-mk-secondary">
                  Embedding Service URL
                  <input
                    class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                    value={props.aiConfig.embedding_service_url}
                    onInput={(e) => props.onAiConfigChange({ ...props.aiConfig, embedding_service_url: e.currentTarget.value })}
                  />
                </label>
                <label class="text-[12px] text-mk-secondary">
                  Embedding Model
                  <input
                    class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                    value={props.aiConfig.embedding_model}
                    onInput={(e) => props.onAiConfigChange({ ...props.aiConfig, embedding_model: e.currentTarget.value })}
                  />
                </label>
              </div>

              <div class="mt-2 grid grid-cols-1 sm:grid-cols-3 gap-2.5">
                <label class="text-[12px] text-mk-secondary">
                  Temperature
                  <input
                    type="number"
                    step="0.1"
                    min="0"
                    max="2"
                    class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                    value={String(props.aiConfig.temperature)}
                    onInput={(e) => props.onAiConfigChange({ ...props.aiConfig, temperature: Number(e.currentTarget.value) || 0 })}
                  />
                </label>
                <label class="text-[12px] text-mk-secondary">
                  Max Tokens
                  <input
                    type="number"
                    min="64"
                    class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                    value={String(props.aiConfig.max_tokens)}
                    onInput={(e) => props.onAiConfigChange({ ...props.aiConfig, max_tokens: Number(e.currentTarget.value) || 256 })}
                  />
                </label>
                <label class="text-[12px] text-mk-secondary">
                  Timeout (ms)
                  <input
                    type="number"
                    min="1000"
                    class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                    value={String(props.aiConfig.timeout_ms)}
                    onInput={(e) => props.onAiConfigChange({ ...props.aiConfig, timeout_ms: Number(e.currentTarget.value) || 30000 })}
                  />
                </label>
              </div>

              <div class="mt-3 flex flex-wrap items-center gap-2">
                <button
                  class="px-3 py-1.5 rounded-md text-[12px] font-semibold bg-mk-green hover:bg-mk-green-hover transition-colors"
                  style={{ color: "var(--mk-sidebar)" }}
                  disabled={props.aiBusy}
                  onClick={props.onSaveAiConfig}
                >
                  Save AI Settings
                </button>
                <button
                  class="px-3 py-1.5 rounded-md text-[12px] font-medium text-mk-secondary border border-mk-separator hover:bg-mk-fill transition-colors"
                  disabled={props.aiBusy}
                  onClick={props.onRefreshOllamaModels}
                >
                  Refresh Models
                </button>
                <button
                  class="px-3 py-1.5 rounded-md text-[12px] font-medium text-mk-secondary border border-mk-separator hover:bg-mk-fill transition-colors"
                  disabled={props.aiBusy}
                  onClick={props.onCheckOllama}
                >
                  Test Ollama
                </button>
                <button
                  class="px-3 py-1.5 rounded-md text-[12px] font-medium text-mk-secondary border border-mk-separator hover:bg-mk-fill transition-colors"
                  disabled={props.aiBusy}
                  onClick={props.onCheckEmbedding}
                >
                  Test Embeddings
                </button>
                <button
                  class="px-3 py-1.5 rounded-md text-[12px] font-medium text-mk-secondary border border-mk-separator hover:bg-mk-fill transition-colors"
                  disabled={props.aiBusy}
                  onClick={props.onIndexJobs}
                >
                  Index Jobs
                </button>
              </div>

              <div class="mt-2 space-y-1">
                <Show when={props.ollamaStatus}>
                  <p class="text-[12px] text-mk-secondary">{props.ollamaStatus}</p>
                </Show>
                <Show when={props.embeddingStatus}>
                  <p class="text-[12px] text-mk-secondary">{props.embeddingStatus}</p>
                </Show>
                <Show when={props.indexStatus}>
                  <p class="text-[12px] text-mk-secondary">{props.indexStatus}</p>
                </Show>
              </div>

              <div class="mt-4 border-t border-mk-separator pt-3">
                <h4 class="text-[13px] font-semibold text-mk-text">Resume Embedding</h4>
                <label class="block text-[12px] text-mk-secondary mt-2">
                  Resume File (`.pdf`, `.docx`, `.txt`)
                  <div class="mt-1 flex items-stretch gap-2">
                    <input
                      class="flex-1 min-w-0 rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                      value={props.resumeFilePath}
                      onInput={(e) => props.onResumeFilePathChange(e.currentTarget.value)}
                      placeholder="Click Browse… or paste a file path"
                    />
                    <button
                      type="button"
                      class="shrink-0 px-3 rounded-md text-[12px] font-medium text-mk-secondary border border-mk-separator hover:bg-mk-fill transition-colors"
                      disabled={props.aiBusy}
                      onClick={props.onBrowseResumeFile}
                      title="Open native file picker"
                    >
                      Browse…
                    </button>
                  </div>
                </label>
                <div class="mt-2 flex flex-wrap items-center gap-2">
                  <button
                    class="px-3 py-1.5 rounded-md text-[12px] font-medium text-mk-secondary border border-mk-separator hover:bg-mk-fill transition-colors"
                    disabled={props.aiBusy || !props.resumeFilePath.trim()}
                    onClick={props.onUploadResumeFromPath}
                  >
                    Upload Resume
                  </button>
                  <button
                    class="px-3 py-1.5 rounded-md text-[12px] font-medium text-mk-secondary border border-mk-separator hover:bg-mk-fill transition-colors"
                    disabled={props.aiBusy || props.selectedResumeId === null}
                    onClick={props.onIndexResume}
                  >
                    Index Selected Resume
                  </button>
                </div>

                <div class="mt-2">
                  <label class="text-[12px] text-mk-secondary">
                    Resume Profiles
                    <select
                      class="mt-1 w-full rounded-md border border-mk-separator bg-mk-fill px-2.5 py-1.5 text-[12px] text-mk-text outline-none"
                      value={props.selectedResumeId === null ? "" : String(props.selectedResumeId)}
                      onChange={(e) => props.onSelectResume(Number(e.currentTarget.value))}
                    >
                      <option value="" disabled>Select a resume profile</option>
                      <For each={props.resumes}>
                        {(resume) => (
                          <option value={resume.id}>
                            {resume.name}{resume.is_active ? " (Active)" : ""}
                          </option>
                        )}
                      </For>
                    </select>
                  </label>
                </div>
                <Show when={props.resumeStatus}>
                  <p class="mt-2 text-[12px] text-mk-secondary">{props.resumeStatus}</p>
                </Show>
              </div>
            </div>

            <p class="text-[12px] text-mk-tertiary">
              This panel is now the central home for all app configuration and upcoming advanced options.
            </p>
          </div>
        </section>
      </div>
    </Show>
  );
}
