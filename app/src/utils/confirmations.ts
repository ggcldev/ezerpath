export function tryClearAll(confirmFn: (message: string) => boolean, onClearAll: () => void): boolean {
  const approved = confirmFn("Clear all scan history and jobs? This cannot be undone.");
  if (approved) onClearAll();
  return approved;
}

