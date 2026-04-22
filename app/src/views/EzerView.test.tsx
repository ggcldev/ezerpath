import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "solid-js/web";

const invokeMock = vi.fn();
const openAllowlistedHttpsUrlMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    startDragging: vi.fn(),
  }),
}));

vi.mock("lucide-solid", () => ({
  Bot: () => null,
  MessageSquarePlus: () => null,
  SendHorizontal: () => null,
  Sparkles: () => null,
  Trash2: () => null,
}));

vi.mock("../utils/viewMotion", () => ({
  animateViewEnter: vi.fn(),
}));

vi.mock("../utils/safeOpenUrl", () => ({
  openAllowlistedHttpsUrl: openAllowlistedHttpsUrlMock,
}));

vi.mock("solid-toast", () => ({
  default: {
    error: toastErrorMock,
    success: vi.fn(),
  },
}));

vi.mock("../components/ConfirmModal", () => ({
  default: () => null,
}));

function click(element: Element | null) {
  if (!(element instanceof HTMLElement)) {
    throw new Error("expected HTMLElement");
  }
  element.dispatchEvent(new MouseEvent("click", { bubbles: true }));
}

function findConversationRow(container: HTMLElement, title: string): HTMLElement | null {
  const titleNode = Array.from(container.querySelectorAll("p")).find(
    (el) => el.textContent?.trim() === title,
  );
  return (titleNode?.parentElement?.parentElement as HTMLElement | null) ?? null;
}

async function flush() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

describe("EzerView", () => {
  let container: HTMLDivElement;
  let dispose: (() => void) | undefined;
  let originalAnimate: typeof HTMLElement.prototype.animate | undefined;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    invokeMock.mockReset();
    openAllowlistedHttpsUrlMock.mockReset();
    toastErrorMock.mockReset();
    originalAnimate = HTMLElement.prototype.animate;
    HTMLElement.prototype.animate = vi.fn(() => ({
      cancel: vi.fn(),
    })) as unknown as typeof HTMLElement.prototype.animate;
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: vi.fn().mockReturnValue({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      }),
    });
  });

  afterEach(() => {
    dispose?.();
    dispose = undefined;
    container.remove();
    if (originalAnimate) {
      HTMLElement.prototype.animate = originalAnimate;
    }
  });

  it("ignores stale conversation responses after fast switching", async () => {
    const convoOne = deferred<
      Array<{ id: number; conversation_id: number; role: "assistant"; content: string; created_at: string; meta_json: string }>
    >();
    const convoTwo = deferred<
      Array<{ id: number; conversation_id: number; role: "assistant"; content: string; created_at: string; meta_json: string }>
    >();

    invokeMock.mockImplementation(async (command: string, args?: { conversationId?: number }) => {
      switch (command) {
        case "ai_list_conversations":
          return [
            { id: 1, title: "First chat", created_at: "2026-01-01T00:00:00.000Z", updated_at: "2026-01-01T00:00:00.000Z" },
            { id: 2, title: "Second chat", created_at: "2026-01-02T00:00:00.000Z", updated_at: "2026-01-02T00:00:00.000Z" },
          ];
        case "ai_get_conversation":
          return args?.conversationId === 1 ? convoOne.promise : convoTwo.promise;
        default:
          return null;
      }
    });

    const { default: EzerView } = await import("./EzerView");
    dispose = render(() => <EzerView />, container);

    await flush();

    click(findConversationRow(container, "First chat"));
    click(findConversationRow(container, "Second chat"));
    await flush();

    convoTwo.resolve([
      {
        id: 22,
        conversation_id: 2,
        role: "assistant",
        content: "Second response wins",
        created_at: "2026-01-02T00:00:01.000Z",
        meta_json: "{}",
      },
    ]);
    await flush();

    convoOne.resolve([
      {
        id: 11,
        conversation_id: 1,
        role: "assistant",
        content: "Stale first response",
        created_at: "2026-01-01T00:00:01.000Z",
        meta_json: "{}",
      },
    ]);
    await flush();

    expect(container.textContent).toContain("Second response wins");
    expect(container.textContent).not.toContain("Stale first response");
  });

  it("opens card URLs through the allowlisted opener helper", async () => {
    openAllowlistedHttpsUrlMock.mockResolvedValue(true);
    invokeMock.mockImplementation(async (command: string, args?: { conversationId?: number }) => {
      switch (command) {
        case "ai_list_conversations":
          return [
            { id: 1, title: "Chat", created_at: "2026-01-01T00:00:00.000Z", updated_at: "2026-01-01T00:00:00.000Z" },
          ];
        case "ai_get_conversation":
          return [
            {
              id: 33,
              conversation_id: args?.conversationId ?? 1,
              role: "assistant",
              content: "Here is a card",
              created_at: "2026-01-01T00:00:01.000Z",
              meta_json: JSON.stringify({
                cards: [
                  {
                    job_id: 9,
                    title: "SEO Writer",
                    company: "Acme",
                    pay: "$8/hr",
                    posted_at: "2026-01-01",
                    url: "https://www.onlinejobs.ph/jobseekers/job/9",
                    logo_url: "",
                  },
                ],
              }),
            },
          ];
        default:
          return null;
      }
    });

    const { default: EzerView } = await import("./EzerView");
    dispose = render(() => <EzerView />, container);

    await flush();
    click(findConversationRow(container, "Chat"));
    await flush();

    click(Array.from(container.querySelectorAll("button")).find((el) => el.textContent?.includes("SEO Writer")) ?? null);

    expect(openAllowlistedHttpsUrlMock).toHaveBeenCalledWith("https://www.onlinejobs.ph/jobseekers/job/9");
    expect(toastErrorMock).not.toHaveBeenCalled();
  });
});
