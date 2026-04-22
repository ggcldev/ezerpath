import { describe, expect, it, vi } from "vitest";
import { openAllowlistedHttpsUrl, toAllowlistedHttpsUrl } from "./safeOpenUrl";

describe("safeOpenUrl", () => {
  it("opens allowlisted https job URLs through the opener plugin", async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);

    const opened = await openAllowlistedHttpsUrl(
      " https://www.onlinejobs.ph/jobseekers/job/123 ",
      invoke,
    );

    expect(opened).toBe(true);
    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("plugin:opener|open_url", {
      url: "https://www.onlinejobs.ph/jobseekers/job/123",
    });
  });

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

  it("rejects non-https and host-spoofed URLs", () => {
    expect(toAllowlistedHttpsUrl("http://www.onlinejobs.ph/jobseekers/job/123")).toBeNull();
    expect(
      toAllowlistedHttpsUrl(
        "https://example.com/?redirect=https://www.onlinejobs.ph/jobseekers/job/123",
      ),
    ).toBeNull();
    expect(
      toAllowlistedHttpsUrl("https://example.com/www.bruntworkcareers.co/jobs/51936545689"),
    ).toBeNull();
  });
});
