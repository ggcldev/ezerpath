import { describe, expect, it } from "vitest";
import { shouldApplyConversationResponse } from "./conversationLoad";

describe("shouldApplyConversationResponse", () => {
  it("rejects stale responses after fast conversation switching", () => {
    expect(shouldApplyConversationResponse(1, 2, 1, 2)).toBe(false);
  });

  it("accepts the latest response for the selected conversation", () => {
    expect(shouldApplyConversationResponse(3, 3, 4, 4)).toBe(true);
  });
});
