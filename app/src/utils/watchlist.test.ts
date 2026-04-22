import { describe, expect, it, vi } from "vitest";
import { loadWatchlistJobs } from "./watchlist";

describe("loadWatchlistJobs", () => {
  it("uses the dedicated watchlist command without a date-range filter", async () => {
    const invoke = vi.fn().mockResolvedValue([{ id: 1, watchlisted: true }]);

    const rows = await loadWatchlistJobs(invoke);

    expect(rows).toEqual([{ id: 1, watchlisted: true }]);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("get_watchlisted_jobs");
  });
});
