import { invoke } from "@tauri-apps/api/core";

type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export async function loadWatchlistJobs<T>(
  invokeFn: InvokeFn = invoke as InvokeFn,
): Promise<T[]> {
  return invokeFn<T[]>("get_watchlisted_jobs");
}
