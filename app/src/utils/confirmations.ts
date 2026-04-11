export function tryClearAll(
  confirmFn: (message: string) => boolean,
  onClearAll: () => void | Promise<void>
): boolean {
  const approved = confirmFn("Clear all scan history and jobs? This cannot be undone.");
  if (approved) void onClearAll();
  return approved;
}
