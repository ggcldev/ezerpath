import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Bot, MessageSquarePlus, SendHorizontal, Sparkles, Trash2 } from "lucide-solid";
import { animate } from "motion";
import { animateViewEnter } from "../utils/viewMotion";
import { shouldApplyConversationResponse } from "../utils/conversationLoad";
import { openAllowlistedHttpsUrl } from "../utils/safeOpenUrl";
import toast from "solid-toast";
import ConfirmModal from "../components/ConfirmModal";

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

interface AiJobCard {
  job_id: number;
  title: string;
  company: string;
  pay: string;
  posted_at: string;
  url: string;
  logo_url: string;
}

interface AiMessageMeta {
  provider?: string;
  scope?: string;
  cards?: AiJobCard[];
  error_code?: string;
}

function errorBadgeLabel(code: string): string {
  switch (code) {
    case "NO_MATCHES":
      return "No matches";
    case "INSUFFICIENT_DATA":
      return "Partial data";
    case "AMBIGUOUS_REFERENCE":
      return "Unclear reference";
    case "MISSING_LINKED_RESULTS":
      return "Referenced jobs unavailable";
    case "MODEL_ERROR":
      return "Model error";
    default:
      return code;
  }
}

interface AiChatError {
  code: string;
  message: string;
}

interface AiChatResponse {
  conversation_id: number;
  reply: string;
  cards?: AiJobCard[] | null;
  error?: AiChatError | null;
}

interface ConfirmDialogState {
  title: string;
  description: string;
  confirmText: string;
  destructive?: boolean;
  onConfirm: () => Promise<void>;
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

function TypewriterText(props: {
  text: string;
  active: boolean;
  onDone?: () => void;
}) {
  const [display, setDisplay] = createSignal(props.active ? "" : props.text);

  createEffect(() => {
    if (!props.active) {
      setDisplay(props.text);
      return;
    }

    setDisplay("");
    let index = 0;
    const total = props.text.length;
    const timer = window.setInterval(() => {
      if (index >= total) {
        window.clearInterval(timer);
        props.onDone?.();
        return;
      }
      const step = total > 900 ? 6 : total > 500 ? 5 : total > 220 ? 3 : 2;
      index = Math.min(index + step, total);
      setDisplay(props.text.slice(0, index));
    }, 12);

    onCleanup(() => window.clearInterval(timer));
  });

  return <p class="text-[14px] leading-7 text-mk-text whitespace-pre-wrap break-words">{display()}</p>;
}

export default function EzerView() {
  const [conversations, setConversations] = createSignal<AiConversation[]>([]);
  const [messages, setMessages] = createSignal<AiMessage[]>([]);
  const [selectedConversationId, setSelectedConversationId] = createSignal<number | null>(null);
  const [draft, setDraft] = createSignal("");
  const [sending, setSending] = createSignal(false);
  const [localError, setLocalError] = createSignal("");
  const [confirmDialog, setConfirmDialog] = createSignal<ConfirmDialogState | null>(null);
  const [confirmBusy, setConfirmBusy] = createSignal(false);
  const [typingMessageId, setTypingMessageId] = createSignal<number | null>(null);
  const [typingDone, setTypingDone] = createSignal(true);

  let viewEl!: HTMLDivElement;
  let messagesEl!: HTMLDivElement;
  let textareaEl!: HTMLTextAreaElement;
  let latestConversationLoadToken = 0;

  const animateMessageEnter = (el: HTMLElement, role: AiMessage["role"]) => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const fromY = role === "user" ? 8 : 10;
    const duration = role === "user" ? 0.2 : 0.24;
    const delay = role === "assistant" ? 0.04 : 0;
    animate(
      el,
      { opacity: [0, 1], transform: [`translateY(${fromY}px)`, "translateY(0px)"] },
      { duration, delay, easing: [0.2, 0.9, 0.25, 1] }
    );
  };

  const parseMeta = (raw: string): AiMessageMeta => {
    if (!raw) return {};
    try {
      const parsed = JSON.parse(raw) as AiMessageMeta;
      if (!parsed || typeof parsed !== "object") return {};
      return parsed;
    } catch {
      return {};
    }
  };

  const openUrl = (rawUrl: string) => {
    void openAllowlistedHttpsUrl(rawUrl)
      .then((opened) => {
        if (!opened) {
          toast.error("Invalid job URL.");
        }
      })
      .catch(() => toast.error("Could not open job URL."));
  };

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
    const requestToken = ++latestConversationLoadToken;
    const list = await invoke<AiMessage[]>("ai_get_conversation", { conversationId });
    if (!shouldApplyConversationResponse(
      conversationId,
      selectedConversationId(),
      requestToken,
      latestConversationLoadToken,
    )) {
      return [];
    }
    setMessages(list);
    return list;
  };

  const startNewChat = () => {
    latestConversationLoadToken += 1;
    setSelectedConversationId(null);
    setMessages([]);
    setDraft("");
    setLocalError("");
    setTypingMessageId(null);
    setTypingDone(true);
    queueMicrotask(() => textareaEl?.focus());
  };

  const deleteConversation = async (conversationId: number) => {
    try {
      await invoke("ai_delete_conversation", { conversationId });
      if (selectedConversationId() === conversationId) {
        latestConversationLoadToken += 1;
        setSelectedConversationId(null);
        setMessages([]);
        setTypingMessageId(null);
        setTypingDone(true);
      }
      await loadConversations();
      toast.success("Chat deleted.");
    } catch (e: any) {
      setLocalError(String(e));
      toast.error("Failed to delete chat.");
    }
  };

  const clearAllConversations = async () => {
    try {
      await invoke("ai_clear_conversations");
      latestConversationLoadToken += 1;
      setSelectedConversationId(null);
      setMessages([]);
      setTypingMessageId(null);
      setTypingDone(true);
      await loadConversations();
      toast.success("All Ezer chat history cleared.");
    } catch (e: any) {
      setLocalError(String(e));
      toast.error("Failed to clear chat history.");
    }
  };

  const requestDeleteConversation = (conversationId: number) => {
    setConfirmDialog({
      title: "Delete this chat?",
      description: "This will permanently remove this Ezer conversation.",
      confirmText: "Delete chat",
      destructive: true,
      onConfirm: async () => deleteConversation(conversationId),
    });
  };

  const requestClearAllConversations = () => {
    setConfirmDialog({
      title: "Clear all chat history?",
      description: "This will permanently remove all Ezer conversations. This action cannot be undone.",
      confirmText: "Clear all",
      destructive: true,
      onConfirm: clearAllConversations,
    });
  };

  const handleConfirmModal = async () => {
    const dialog = confirmDialog();
    if (!dialog || confirmBusy()) return;
    setConfirmBusy(true);
    try {
      await dialog.onConfirm();
      setConfirmDialog(null);
    } finally {
      setConfirmBusy(false);
    }
  };

  const sendMessage = async () => {
    const text = draft().trim();
    if (!text || sending()) return;

    const tempMessageId = -Date.now();
    const optimisticUserMessage: AiMessage = {
      id: tempMessageId,
      conversation_id: selectedConversationId() ?? -1,
      role: "user",
      content: text,
      created_at: new Date().toISOString(),
      meta_json: "{}",
    };

    setMessages((prev) => [...prev, optimisticUserMessage]);
    setDraft("");
    setSending(true);
    setLocalError("");

    try {
      const response = await invoke<AiChatResponse>("ai_chat", {
        conversationId: selectedConversationId(),
        message: text,
        filters: null,
      });
      setSelectedConversationId(response.conversation_id);
      const [, latestMessages] = await Promise.all([loadConversations(), loadMessages(response.conversation_id)]);
      const latestAssistant = [...latestMessages].reverse().find((m) => m.role === "assistant");
      if (latestAssistant) {
        setTypingMessageId(latestAssistant.id);
        setTypingDone(false);
      } else {
        setTypingMessageId(null);
        setTypingDone(true);
      }
      queueMicrotask(() => textareaEl?.focus());
    } catch (e: any) {
      setMessages((prev) => prev.filter((m) => m.id !== tempMessageId));
      setDraft(text);
      setLocalError(String(e));
      setTypingMessageId(null);
      setTypingDone(true);
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

      <div class="flex-1 min-h-0 grid grid-cols-[240px_minmax(0,1fr)]">
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
            <button
              class="mt-2 w-full inline-flex items-center justify-center gap-1.5 px-2 py-2 rounded-lg text-[12px] font-medium border border-mk-separator text-mk-secondary hover:bg-mk-fill hover:text-mk-pink transition-colors"
              onClick={requestClearAllConversations}
            >
              <Trash2 class="w-3.5 h-3.5" />
              Clear All
            </button>
          </div>

          <div class="flex-1 overflow-auto p-2 space-y-1">
            <For each={conversations()} fallback={<p class="px-2 py-2 text-[12px] text-mk-tertiary">No chats yet</p>}>
              {(c) => (
                <div
                  class={`w-full text-left px-2.5 py-2 rounded-lg transition-colors ${
                    selectedConversationId() === c.id
                      ? "bg-mk-fill text-mk-text border border-mk-separator"
                      : "hover:bg-mk-fill text-mk-secondary border border-transparent cursor-pointer"
                  }`}
                  onClick={async () => {
                    setSelectedConversationId(c.id);
                    await loadMessages(c.id);
                    setTypingMessageId(null);
                    setTypingDone(true);
                  }}
                >
                  <div class="flex items-start justify-between gap-2">
                    <div class="min-w-0">
                      <p class="text-[12px] font-medium truncate">{c.title || "Ezer Chat"}</p>
                      <p class="text-[10px] text-mk-tertiary mt-0.5">{prettyDate(c.updated_at)}</p>
                    </div>
                    <button
                      class="shrink-0 w-6 h-6 inline-flex items-center justify-center rounded-md text-mk-tertiary hover:text-mk-pink hover:bg-mk-fill"
                      aria-label="Delete chat"
                      onClick={(e) => {
                        e.stopPropagation();
                        requestDeleteConversation(c.id);
                      }}
                    >
                      <Trash2 class="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              )}
            </For>
          </div>
        </aside>

        <section
          class="min-h-0 min-w-0 flex flex-col relative"
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
            <div ref={messagesEl!} class="flex-1 min-h-0 overflow-auto px-6 py-4 space-y-2.5">
              <For each={messages()}>
                {(m) => (
                  <div
                    ref={(el) => animateMessageEnter(el, m.role)}
                    class={`max-w-[min(52%,30rem)] rounded-xl px-3 py-2 ${
                      m.role === "user"
                        ? "ml-auto bg-mk-green-dim border border-mk-green-dim"
                        : "bg-mk-grouped-bg border border-mk-separator"
                    }`}
                  >
                    <p class="text-[10px] font-semibold text-mk-tertiary mb-0.5">
                      {m.role === "user" ? "You" : m.role === "assistant" ? "Ezer" : "System"}
                    </p>
                    <Show
                      when={m.role === "assistant" && typingMessageId() === m.id && !typingDone()}
                      fallback={<p class="text-[13px] leading-[1.45] text-mk-text whitespace-pre-wrap break-words">{m.content}</p>}
                    >
                      <TypewriterText
                        text={m.content}
                        active
                        onDone={() => {
                          if (typingMessageId() === m.id) setTypingDone(true);
                        }}
                      />
                    </Show>
                    <Show
                      when={
                        m.role === "assistant" &&
                        (parseMeta(m.meta_json).cards?.length || 0) > 0 &&
                        !(typingMessageId() === m.id && !typingDone())
                      }
                    >
                      <div class="mt-3 space-y-2">
                        <For each={parseMeta(m.meta_json).cards || []}>
                          {(card) => (
                            <button
                              class="w-full text-left rounded-xl border border-mk-separator bg-mk-bg/55 px-3 py-2.5 hover:bg-mk-fill transition-colors"
                              onClick={() => openUrl(card.url)}
                            >
                              <div class="flex items-start justify-between gap-3">
                                <div class="min-w-0">
                                  <p class="text-[13px] font-semibold text-mk-text truncate">{card.title}</p>
                                  <p class="text-[12px] text-mk-secondary truncate mt-0.5">{card.company}</p>
                                  <p class="text-[11px] text-mk-tertiary mt-1">
                                    Pay: {card.pay || "-"} | Posted: {card.posted_at || "-"}
                                  </p>
                                </div>
                                <Show when={Boolean(card.logo_url)}>
                                  <img
                                    src={card.logo_url}
                                    alt={card.company}
                                    class="w-8 h-8 rounded-md object-cover border border-mk-separator shrink-0"
                                  />
                                </Show>
                              </div>
                            </button>
                          )}
                        </For>
                      </div>
                    </Show>
                    <Show
                      when={
                        m.role === "assistant" &&
                        parseMeta(m.meta_json).error_code &&
                        !(typingMessageId() === m.id && !typingDone())
                      }
                    >
                      <div class="mt-2 inline-flex items-center gap-1.5 rounded-full border border-mk-separator bg-mk-grouped-bg/60 px-2 py-0.5 text-[11px] text-mk-tertiary">
                        <span class="w-1.5 h-1.5 rounded-full bg-mk-tertiary" />
                        <span>{errorBadgeLabel(parseMeta(m.meta_json).error_code!)}</span>
                      </div>
                    </Show>
                  </div>
                )}
              </For>

              <Show when={sending()}>
                <div class="max-w-[min(52%,30rem)] rounded-xl px-3 py-2 bg-mk-grouped-bg border border-mk-separator">
                  <p class="text-[10px] font-semibold text-mk-tertiary mb-0.5">Ezer</p>
                  <div class="inline-flex items-center gap-2 text-[12px] text-mk-secondary">
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

      <ConfirmModal
        open={!!confirmDialog()}
        title={confirmDialog()?.title ?? "Confirm action"}
        description={confirmDialog()?.description ?? ""}
        confirmText={confirmDialog()?.confirmText ?? "Confirm"}
        destructive={confirmDialog()?.destructive ?? false}
        busy={confirmBusy()}
        onCancel={() => !confirmBusy() && setConfirmDialog(null)}
        onConfirm={() => void handleConfirmModal()}
      />
    </div>
  );
}
