import { describe, expect, it } from "vitest";
import { filterJobsByScope, getLatestRunId, latestRunCount } from "./jobs";

describe("latest scan helpers", () => {
  const jobs = [
    { id: 1, run_id: 2 },
    { id: 2, run_id: 1 },
    { id: 3, run_id: 2 },
    { id: 4, run_id: null },
  ];
  const runs = [{ id: 2 }, { id: 1 }];

  it("resolves latest run id from runs list", () => {
    expect(getLatestRunId(runs)).toBe(2);
    expect(getLatestRunId([])).toBeNull();
  });

  it("keeps count/list consistent for latest scope", () => {
    const latestId = getLatestRunId(runs);
    const list = filterJobsByScope(jobs, "latest", latestId);
    const count = latestRunCount(jobs, latestId);

    expect(list.map((j) => j.id)).toEqual([1, 3]);
    expect(count).toBe(list.length);
  });

  it("returns full list for all scope", () => {
    expect(filterJobsByScope(jobs, "all", 2)).toHaveLength(jobs.length);
  });
});

