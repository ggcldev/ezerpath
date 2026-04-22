export function shouldApplyConversationResponse(
  requestedConversationId: number,
  selectedConversationId: number | null,
  requestToken: number,
  latestToken: number,
): boolean {
  return requestedConversationId === selectedConversationId && requestToken === latestToken;
}
