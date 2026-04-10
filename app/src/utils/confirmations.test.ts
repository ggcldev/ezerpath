import { describe, expect, it, vi } from "vitest";
import { tryClearAll } from "./confirmations";

describe("tryClearAll", () => {
  it("does not clear when confirmation is denied", () => {
    const onClearAll = vi.fn();
    const confirmed = tryClearAll(() => false, onClearAll);
    expect(confirmed).toBe(false);
    expect(onClearAll).not.toHaveBeenCalled();
  });

  it("clears when confirmation is accepted", () => {
    const onClearAll = vi.fn();
    const confirmed = tryClearAll(() => true, onClearAll);
    expect(confirmed).toBe(true);
    expect(onClearAll).toHaveBeenCalledOnce();
  });
});

