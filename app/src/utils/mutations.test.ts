import { describe, expect, it, vi } from "vitest";
import { runMutation } from "./mutations";

describe("runMutation", () => {
  it("clears error and calls success callback when operation succeeds", async () => {
    const setError = vi.fn();
    const onSuccess = vi.fn();

    const ok = await runMutation(async () => 123, onSuccess, setError);

    expect(ok).toBe(true);
    expect(onSuccess).toHaveBeenCalledOnce();
    expect(setError).toHaveBeenCalledWith("");
  });

  it("sets error and does not call success callback when operation fails", async () => {
    const setError = vi.fn();
    const onSuccess = vi.fn();
    const err = new Error("invoke failed");

    const ok = await runMutation(async () => {
      throw err;
    }, onSuccess, setError);

    expect(ok).toBe(false);
    expect(onSuccess).not.toHaveBeenCalled();
    expect(setError).toHaveBeenCalledWith(String(err));
  });
});

