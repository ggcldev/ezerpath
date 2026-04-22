import { describe, expect, it, vi } from "vitest";
import { openAllowlistedHttpsUrl, toAllowlistedHttpsUrl } from "./safeOpenUrl";

describe("safeOpenUrl", () => {
  it("rejects invalid AI card URLs before opener invocation", async () => {
    const invoke = vi.fn();

    const opened = await openAllowlistedHttpsUrl("javascript:alert(1)", invoke);

    expect(opened).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("normalizes allowlisted https job URLs", () => {
    expect(toAllowlistedHttpsUrl(" https://www.onlinejobs.ph/jobseekers/job/123 ")).toBe(
      "https://www.onlinejobs.ph/jobseekers/job/123",
    );
  });
});
