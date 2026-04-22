import { describe, expect, it } from "vitest";
import { shouldApplyConversationResponse } from "./conversationLoad";

describe("shouldApplyConversationResponse", () => {
  it("rejects stale responses after fast conversation switching", () => {
    expect(shouldApplyConversationResponse(1, 2, 1, 2)).toBe(false);
  });

  it("rejects an older request token for the same conversation", () => {
    expect(shouldApplyConversationResponse(3, 3, 4, 5)).toBe(false);
  });

  it("rejects responses after the selected conversation is cleared", () => {
    expect(shouldApplyConversationResponse(3, null, 4, 4)).toBe(false);
  });

  it("accepts the latest response for the selected conversation", () => {
    expect(shouldApplyConversationResponse(3, 3, 4, 4)).toBe(true);
  });
});
