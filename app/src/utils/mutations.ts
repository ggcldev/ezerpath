export async function runMutation(
  operation: () => Promise<unknown>,
  onSuccess: () => void,
  setError: (message: string) => void
): Promise<boolean> {
  try {
    await operation();
    setError("");
    onSuccess();
    return true;
  } catch (error) {
    setError(String(error));
    return false;
  }
}

