import { createEffect, createSignal, For, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Bot, MessageSquarePlus, SendHorizontal, Sparkles } from "lucide-solid";
import { animateViewEnter } from "../utils/viewMotion";

interface AiConversation {
  id: number;
  title: string;
  created_at: string;
  updated_at: string;
}

interface AiMessage {
  id: number;
  conversation_id: number;
  role: "user" | "assistant" | "system";
  content: string;
  created_at: string;
  meta_json: string;
}

interface AiChatResponse {
  conversation_id: number;
  reply: string;
}

function prettyDate(raw: string): string {
  const d = new Date(raw);
  if (isNaN(d.getTime())) return raw;
  return d.toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  });
}

export default function EzerView() {
  const [conversations, setConversations] = createSignal<AiConversation[]>([]);
  const [messages, setMessages] = createSignal<AiMessage[]>([]);
  const [selectedConversationId, setSelectedConversationId] = createSignal<number | null>(null);
  const [draft, setDraft] = createSignal("");
  const [sending, setSending] = createSignal(false);
  const [localError, setLocalError] = createSignal("");

  let viewEl!: HTMLDivElement;
  let messagesEl!: HTMLDivElement;
  let textareaEl!: HTMLTextAreaElement;

  const handleWindowDrag = (e: MouseEvent) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest("button,input,a,textarea,select,[role='button']")) return;
    void getCurrentWindow().startDragging();
  };

  const loadConversations = async () => {
    const list = await invoke<AiConversation[]>("ai_list_conversations");
    setConversations(list);
  };

  const loadMessages = async (conversationId: number) => {
    const list = await invoke<AiMessage[]>("ai_get_conversation", { conversationId });
    setMessages(list);
  };

  const startNewChat = () => {
    setSelectedConversationId(null);
    setMessages([]);
    setDraft("");
    setLocalError("");
    queueMicrotask(() => textareaEl?.focus());
  };

  const sendMessage = async () => {
    const text = draft().trim();
    if (!text || sending()) return;
    setSending(true);
    setLocalError("");
    try {
      const response = await invoke<AiChatResponse>("ai_chat", {
        conversationId: selectedConversationId(),
        message: text,
        filters: null,
      });
      setDraft("");
      setSelectedConversationId(response.conversation_id);
      await Promise.all([loadConversations(), loadMessages(response.conversation_id)]);
      queueMicrotask(() => textareaEl?.focus());
    } catch (e: any) {
      setLocalError(String(e));
    } finally {
      setSending(false);
    }
  };

  createEffect(() => {
    messages();
    queueMicrotask(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    });
  });

  onMount(async () => {
    animateViewEnter(viewEl);
    try {
      await loadConversations();
      queueMicrotask(() => textareaEl?.focus());
    } catch (e: any) {
      setLocalError(String(e));
    }
  });

  const hasMessages = () => messages().length > 0;

  return (
    <div ref={viewEl!} class="relative flex-1 flex flex-col min-h-0 min-w-0 bg-mk-bg">
      <div class="absolute top-0 left-0 right-0 h-8 z-10" onMouseDown={handleWindowDrag} />

      <div class="flex-1 min-h-0 grid grid-cols-[240px_1fr]">
        <aside class="border-r border-mk-separator min-h-0 flex flex-col bg-mk-grouped-bg/35">
          <div class="px-3 py-3 border-b border-mk-separator">
            <div class="flex items-center gap-2">
              <span class="w-7 h-7 rounded-lg bg-mk-green-dim flex items-center justify-center">
                <Bot class="w-4 h-4 text-mk-green" />
              </span>
              <div>
                <p class="text-[12px] font-semibold text-mk-text">Ezer</p>
                <p class="text-[10px] text-mk-tertiary">Job Copilot</p>
              </div>
            </div>
            <button
              class="mt-3 w-full inline-flex items-center justify-center gap-1.5 px-2 py-2 rounded-lg text-[12px] font-medium border border-mk-separator text-mk-secondary hover:bg-mk-fill hover:text-mk-text transition-colors"
              onClick={startNewChat}
            >
              <MessageSquarePlus class="w-3.5 h-3.5" />
              New Chat
            </button>
          </div>

          <div class="flex-1 overflow-auto p-2 space-y-1">
            <For each={conversations()} fallback={<p class="px-2 py-2 text-[12px] text-mk-tertiary">No chats yet</p>}>
              {(c) => (
                <button
                  class={`w-full text-left px-2.5 py-2 rounded-lg transition-colors ${
                    selectedConversationId() === c.id
                      ? "bg-mk-fill text-mk-text border border-mk-separator"
                      : "hover:bg-mk-fill text-mk-secondary border border-transparent"
                  }`}
                  onClick={async () => {
                    setSelectedConversationId(c.id);
                    await loadMessages(c.id);
                  }}
                >
                  <p class="text-[12px] font-medium truncate">{c.title || "Ezer Chat"}</p>
                  <p class="text-[10px] text-mk-tertiary mt-0.5">{prettyDate(c.updated_at)}</p>
                </button>
              )}
            </For>
          </div>
        </aside>

        <section
          class="min-h-0 flex flex-col relative"
          style={{ background: "color-mix(in oklab, var(--mk-bg) 85%, black 15%)" }}
        >
          <Show
            when={hasMessages()}
            fallback={
              <div class="flex-1 min-h-0 flex flex-col items-center justify-center px-6">
                <div class="max-w-2xl w-full">
                  <div class="mx-auto w-fit px-3 py-1 rounded-full text-[11px] border border-mk-separator bg-mk-grouped-bg text-mk-tertiary inline-flex items-center gap-1.5">
                    <Sparkles class="w-3.5 h-3.5 text-mk-green" />
                    Ezer AI
                  </div>
                  <h2 class="mt-4 text-[30px] leading-tight font-semibold text-mk-text text-center">
                    How can I help with your job search today?
                  </h2>
                  <p class="mt-2 text-[13px] text-mk-tertiary text-center">
                    Ask for top jobs, summaries, keyword suggestions, or resume matching.
                  </p>

                  <div class="mt-6 rounded-2xl border border-mk-separator/70 bg-mk-grouped-bg px-3 py-3 shadow-sm">
                    <textarea
                      ref={textareaEl!}
                      class="flat-focus w-full min-h-[92px] max-h-[180px] resize-none bg-transparent px-1 py-1 text-[14px] text-mk-text outline-none placeholder:text-mk-tertiary"
                      placeholder="Message Ezer..."
                      value={draft()}
                      onInput={(e) => setDraft(e.currentTarget.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && !e.shiftKey) {
                          e.preventDefault();
                          void sendMessage();
                        }
                      }}
                    />
                    <div class="mt-2 flex items-center justify-end">
                      <button
                        class={`h-9 px-3 rounded-lg text-[12px] font-semibold transition-colors ${
                          sending()
                            ? "bg-mk-fill text-mk-tertiary cursor-not-allowed"
                            : "bg-mk-green hover:bg-mk-green-hover"
                        }`}
                        style={!sending() ? { color: "var(--mk-sidebar)" } : {}}
                        disabled={sending()}
                        onClick={() => void sendMessage()}
                      >
                        <span class="inline-flex items-center gap-1">
                          <SendHorizontal class="w-3.5 h-3.5" />
                          {sending() ? "Sending..." : "Send"}
                        </span>
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            }
          >
            <div ref={messagesEl!} class="flex-1 min-h-0 overflow-auto px-6 py-5 space-y-4">
              <For each={messages()}>
                {(m) => (
                  <div class={`max-w-[78%] rounded-2xl px-4 py-3 ${
                    m.role === "user"
                      ? "ml-auto bg-mk-green-dim border border-mk-green-dim"
                      : "bg-mk-grouped-bg border border-mk-separator"
                  }`}>
                    <p class="text-[11px] font-semibold text-mk-tertiary mb-1">
                      {m.role === "user" ? "You" : m.role === "assistant" ? "Ezer" : "System"}
                    </p>
                    <p class="text-[14px] leading-7 text-mk-text whitespace-pre-wrap break-words">{m.content}</p>
                  </div>
                )}
              </For>

              <Show when={sending()}>
                <div class="max-w-[78%] rounded-2xl px-4 py-3 bg-mk-grouped-bg border border-mk-separator">
                  <p class="text-[11px] font-semibold text-mk-tertiary mb-1">Ezer</p>
                  <div class="inline-flex items-center gap-2 text-[13px] text-mk-secondary">
                    <span>Ezer is thinking</span>
                    <span class="inline-flex items-center gap-1">
                      <span class="w-1.5 h-1.5 rounded-full bg-mk-tertiary animate-pulse" />
                      <span class="w-1.5 h-1.5 rounded-full bg-mk-tertiary animate-pulse [animation-delay:120ms]" />
                      <span class="w-1.5 h-1.5 rounded-full bg-mk-tertiary animate-pulse [animation-delay:240ms]" />
                    </span>
                  </div>
                </div>
              </Show>
            </div>

            <div class="border-t border-mk-separator px-6 py-4 bg-mk-bg/92 backdrop-blur-sm">
              <div class="rounded-2xl border border-mk-separator/70 bg-mk-grouped-bg px-3 py-3">
                <textarea
                  ref={textareaEl!}
                  class="flat-focus w-full min-h-[70px] max-h-[160px] resize-none bg-transparent px-1 py-1 text-[14px] text-mk-text outline-none placeholder:text-mk-tertiary"
                  placeholder="Message Ezer..."
                  value={draft()}
                  onInput={(e) => setDraft(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      void sendMessage();
                    }
                  }}
                />
                <div class="mt-2 flex items-center justify-between">
                  <p class="text-[11px] text-mk-tertiary">Enter to send, Shift+Enter for newline</p>
                  <button
                    class={`h-9 px-3 rounded-lg text-[12px] font-semibold transition-colors ${
                      sending()
                        ? "bg-mk-fill text-mk-tertiary cursor-not-allowed"
                        : "bg-mk-green hover:bg-mk-green-hover"
                    }`}
                    style={!sending() ? { color: "var(--mk-sidebar)" } : {}}
                    disabled={sending()}
                    onClick={() => void sendMessage()}
                  >
                    <span class="inline-flex items-center gap-1">
                      <SendHorizontal class="w-3.5 h-3.5" />
                      {sending() ? "Sending..." : "Send"}
                    </span>
                  </button>
                </div>
              </div>
            </div>
          </Show>

          <Show when={localError()}>
            <div class="absolute bottom-3 left-6 right-6 pointer-events-none">
              <p class="text-[12px] text-mk-pink">{localError()}</p>
            </div>
          </Show>
        </section>
      </div>
    </div>
  );
}
