export interface RunLike {
  id: number;
}

export interface JobLike {
  run_id: number | null;
}

export type ScanScope = "all" | "latest";

export function getLatestRunId(runs: RunLike[]): number | null {
  return runs.length > 0 ? runs[0].id : null;
}

export function filterJobsByScope<T extends JobLike>(
  jobs: T[],
  scope: ScanScope,
  latestRunId: number | null
): T[] {
  if (scope !== "latest" || latestRunId === null) return jobs;
  return jobs.filter((job) => job.run_id === latestRunId);
}

export function latestRunCount<T extends JobLike>(jobs: T[], latestRunId: number | null): number {
  if (latestRunId === null) return 0;
  return jobs.filter((job) => job.run_id === latestRunId).length;
}

