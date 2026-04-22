import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "solid-js/web";
import type { Job } from "./types/ipc";

const invokeMock = vi.fn();
const loadWatchlistJobsMock = vi.fn<() => Promise<Job[]>>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("solid-toast", () => ({
  default: {
    success: vi.fn(),
    error: vi.fn(),
    loading: vi.fn(() => "toast-id"),
  },
  Toaster: () => null,
}));

vi.mock("./components/Sidebar", () => ({
  default: (props: {
    onNavigate: (view: "scan" | "watchlist") => void;
  }) => (
    <div>
      <button onClick={() => props.onNavigate("scan")}>Go Scan</button>
      <button onClick={() => props.onNavigate("watchlist")}>Go Watchlist</button>
    </div>
  ),
}));

vi.mock("./components/ConfirmModal", () => ({
  default: () => null,
}));

vi.mock("./components/SettingsPanel", () => ({
  default: () => null,
}));

vi.mock("./views/ScanView", () => ({
  default: (props: { setDateRange: (days: number) => void }) => (
    <button onClick={() => props.setDateRange(14)}>Set 14 days</button>
  ),
}));

vi.mock("./views/JobsView", () => ({
  default: () => null,
}));

vi.mock("./views/EzerView", () => ({
  default: () => null,
}));

vi.mock("./views/WatchlistView", () => ({
  default: (props: { jobs: () => Job[] | undefined }) => (
    <div>
      {(props.jobs() || []).map((job) => (
        <span>{job.title}</span>
      ))}
    </div>
  ),
}));

vi.mock("./utils/watchlist", () => ({
  loadWatchlistJobs: loadWatchlistJobsMock,
}));

function click(element: Element | null) {
  if (!(element instanceof HTMLElement)) {
    throw new Error("expected HTMLElement");
  }
  element.dispatchEvent(new MouseEvent("click", { bubbles: true }));
}

async function flush() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("App watchlist resource", () => {
  let container: HTMLDivElement;
  let dispose: (() => void) | undefined;

  beforeEach(() => {
    container = document.createElement("div");
    document.body.appendChild(container);
    invokeMock.mockReset();
    loadWatchlistJobsMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      switch (command) {
        case "get_keywords":
          return [];
        case "get_runs":
          return [];
        default:
          return null;
      }
    });
    loadWatchlistJobsMock.mockResolvedValue([
      {
        id: 42,
        source: "onlinejobs",
        source_id: "saved-job",
        title: "Saved SEO Role",
        company: "Acme",
        company_logo_url: "",
        pay: "$10/hr",
        posted_at: "2026-01-01T00:00:00.000Z",
        url: "https://www.onlinejobs.ph/jobseekers/job/42",
        summary: "",
        keyword: "seo specialist",
        scraped_at: "2026-01-01T00:00:00.000Z",
        is_new: false,
        watchlisted: true,
        run_id: 1,
        salary_min: null,
        salary_max: null,
        salary_currency: "",
        salary_period: "",
        normalized_pay_usd_hourly: null,
        normalized_pay_usd_monthly: null,
        pay_range: "unspecified",
        applied: false,
        job_type: "Full-time",
      },
    ]);
  });

  afterEach(() => {
    dispose?.();
    dispose = undefined;
    container.remove();
  });

  it("keeps watchlist data independent from scan date-range changes", async () => {
    const { default: App } = await import("./App");
    dispose = render(() => <App />, container);

    await flush();
    expect(loadWatchlistJobsMock).toHaveBeenCalledTimes(1);

    click(Array.from(container.querySelectorAll("button")).find((el) => el.textContent === "Set 14 days") ?? null);
    await flush();

    expect(loadWatchlistJobsMock).toHaveBeenCalledTimes(1);

    click(Array.from(container.querySelectorAll("button")).find((el) => el.textContent === "Go Watchlist") ?? null);
    await flush();

    expect(container.textContent).toContain("Saved SEO Role");
    expect(loadWatchlistJobsMock).toHaveBeenCalledTimes(1);
  });
});
