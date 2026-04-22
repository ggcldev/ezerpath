import { invoke } from "@tauri-apps/api/core";

type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

const ALLOWED_HOSTS = [
  "onlinejobs.ph",
  "www.onlinejobs.ph",
  "bruntworkcareers.co",
  "www.bruntworkcareers.co",
];

export function toAllowlistedHttpsUrl(rawUrl: string): string | null {
  try {
    const parsed = new URL(rawUrl.trim());
    if (parsed.protocol !== "https:") return null;
    if (!ALLOWED_HOSTS.includes(parsed.hostname)) return null;
    return parsed.toString();
  } catch {
    return null;
  }
}

export async function openAllowlistedHttpsUrl(
  rawUrl: string,
  invokeFn: InvokeFn = invoke as InvokeFn,
): Promise<boolean> {
  const safeUrl = toAllowlistedHttpsUrl(rawUrl);
  if (!safeUrl) return false;
  await invokeFn("plugin:opener|open_url", { url: safeUrl });
  return true;
}
