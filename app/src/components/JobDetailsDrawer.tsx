import { Show, createEffect, createSignal, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { Building2, CalendarDays, Copy, ExternalLink, Tag, Wallet, X } from "lucide-solid";

interface JobDetails {
  title: string;
  company: string;
  company_logo_url?: string;
  source: string;
  pay: string;
  posted_at: string;
  keyword: string;
  summary: string;
  url: string;
}

interface JobDetailsDrawerProps {
  job: JobDetails | null;
  onClose: () => void;
  onOpenUrl: (url: string) => void;
}

interface CrawledJobDetails {
  company: string;
  poster_name: string;
  company_logo_url: string;
  description: string;
  description_html: string;
}

function formatPosted(raw: string): string {
  if (!raw) return "-";
  const d = new Date(raw);
  if (isNaN(d.getTime())) return raw;
  const m = String(d.getMonth() + 1).padStart(2, "0");
  return `${m}/${d.getDate()}/${String(d.getFullYear()).slice(2)}`;
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function plainTextToSimpleHtml(value: string): string {
  const text = value.replace(/\r\n/g, "\n").trim();
  if (!text) return "";

  const paragraphs = text.split(/\n{2,}/).map((p) => p.trim()).filter(Boolean);
  return paragraphs
    .map((p) => `<p>${escapeHtml(p).replace(/\n/g, "<br>")}</p>`)
    .join("");
}

function buildDescriptionHtml(crawled: CrawledJobDetails | null, fallbackSummary: string): string {
  const rawHtml = (crawled?.description_html || "").trim();
  const rawText = (crawled?.description || fallbackSummary || "").trim();

  if (rawHtml) {
    return sanitizeDescriptionHtml(rawHtml);
  }

  if (rawText) {
    return sanitizeDescriptionHtml(plainTextToSimpleHtml(rawText));
  }

  return "<p>No description available from the listing preview.</p>";
}

function htmlToPlainText(raw: string): string {
  if (!raw) return "";
  if (typeof document === "undefined") {
    return raw
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/<\/(p|div|h1|h2|h3|h4|h5|h6|blockquote|pre)>/gi, "\n\n")
      .replace(/<li[^>]*>/gi, "\n- ")
      .replace(/<\/li>/gi, "")
      .replace(/<[^>]+>/g, " ")
      .replace(/\r\n/g, "\n")
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .replace(/[ \t]{2,}/g, " ")
      .trim();
  }

  const root = document.createElement("div");
  root.innerHTML = raw;
  const out: string[] = [];

  const push = (text: string) => {
    if (!text) return;
    out.push(text);
  };

  const walkInline = (node: Node): string => {
    if (node.nodeType === Node.TEXT_NODE) return (node.textContent || "");
    if (node.nodeType !== Node.ELEMENT_NODE) return "";
    const el = node as HTMLElement;
    if (el.tagName === "BR") return "\n";
    return Array.from(el.childNodes).map(walkInline).join("");
  };

  const walk = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      const text = (node.textContent || "").replace(/\s+/g, " ");
      push(text);
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const el = node as HTMLElement;
    const tag = el.tagName.toLowerCase();

    if (tag === "br") {
      push("\n");
      return;
    }
    if (tag === "ul") {
      for (const child of Array.from(el.children)) {
        if (child.tagName.toLowerCase() !== "li") continue;
        const itemText = walkInline(child).replace(/\s+/g, " ").trim();
        if (itemText) push(`- ${itemText}\n`);
      }
      push("\n");
      return;
    }
    if (tag === "ol") {
      let index = 1;
      for (const child of Array.from(el.children)) {
        if (child.tagName.toLowerCase() !== "li") continue;
        const itemText = walkInline(child).replace(/\s+/g, " ").trim();
        if (itemText) push(`${index}. ${itemText}\n`);
        index += 1;
      }
      push("\n");
      return;
    }
    if (tag === "li") {
      const itemText = walkInline(el).replace(/\s+/g, " ").trim();
      if (itemText) push(`- ${itemText}\n`);
      return;
    }

    const blockTags = new Set(["p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre"]);
    if (blockTags.has(tag)) {
      const text = walkInline(el).replace(/\s+/g, " ").trim();
      if (text) push(`${text}\n\n`);
      return;
    }

    for (const child of Array.from(el.childNodes)) {
      walk(child);
    }
  };

  for (const child of Array.from(root.childNodes)) {
    walk(child);
  }

  return out
    .join("")
    .replace(/\r\n/g, "\n")
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/[ \t]{2,}/g, " ")
    .trim();
}

function sanitizeDescriptionHtml(raw: string): string {
  if (typeof document === "undefined") return raw;
  const parser = new DOMParser();
  const doc = parser.parseFromString(`<div>${raw}</div>`, "text/html");
  const root = doc.body.firstElementChild as HTMLElement | null;
  if (!root) return "";

  const allowed = new Set([
    "p", "br", "ul", "ol", "li", "strong", "em", "b", "i", "u", "a",
    "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "code",
  ]);

  const walk = (node: Node) => {
    const children = Array.from(node.childNodes);
    for (const child of children) {
      if (child.nodeType === Node.ELEMENT_NODE) {
        const el = child as HTMLElement;
        const tag = el.tagName.toLowerCase();
        if (!allowed.has(tag)) {
          while (el.firstChild) node.insertBefore(el.firstChild, el);
          node.removeChild(el);
          continue;
        }
        for (const attr of Array.from(el.attributes)) {
          const name = attr.name.toLowerCase();
          if (tag === "a" && name === "href") continue;
          el.removeAttribute(attr.name);
        }
        if (tag === "a") {
          const href = (el.getAttribute("href") || "").trim();
          if (!/^https?:\/\//i.test(href)) {
            el.removeAttribute("href");
          } else {
            el.setAttribute("target", "_blank");
            el.setAttribute("rel", "noopener noreferrer");
          }
        }
        walk(el);
      } else if (child.nodeType === Node.COMMENT_NODE) {
        node.removeChild(child);
      }
    }
  };

  walk(root);
  return root.innerHTML.trim();
}

export default function JobDetailsDrawer(props: JobDetailsDrawerProps) {
  const [crawled, setCrawled] = createSignal<CrawledJobDetails | null>(null);
  const [loadingDetails, setLoadingDetails] = createSignal(false);
  const [copied, setCopied] = createSignal(false);

  createEffect(() => {
    if (!props.job) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") props.onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    onCleanup(() => document.removeEventListener("keydown", onKeyDown));
  });

  createEffect(() => {
    const job = props.job;
    if (!job) {
      setCrawled(null);
      setLoadingDetails(false);
      return;
    }

    let cancelled = false;
    setCrawled(null);
    setLoadingDetails(true);

    invoke<CrawledJobDetails>("fetch_job_details", { url: job.url })
      .then((result) => {
        if (cancelled) return;
        setCrawled(result);
      })
      .catch(() => {
        if (cancelled) return;
        setCrawled(null);
      })
      .finally(() => {
        if (cancelled) return;
        setLoadingDetails(false);
      });

    onCleanup(() => {
      cancelled = true;
    });
  });

  const copyJobContext = async () => {
    const job = props.job;
    if (!job) return;
    const renderedHtml = buildDescriptionHtml(crawled(), job.summary);
    const descriptionText = htmlToPlainText(renderedHtml);
    const text = [
      `Job Title: ${job.title || "-"}`,
      `Company: ${crawled()?.company || job.company || "-"}`,
      `Posted By: ${crawled()?.poster_name || "-"}`,
      `Source: ${job.source || "-"}`,
      `Posted: ${formatPosted(job.posted_at)}`,
      `Pay: ${job.pay || "-"}`,
      `Keyword: ${job.keyword || "-"}`,
      `URL: ${job.url || "-"}`,
      "",
      "Description:",
      descriptionText || "-",
    ].join("\n");

    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        const area = document.createElement("textarea");
        area.value = text;
        area.setAttribute("readonly", "");
        area.style.position = "fixed";
        area.style.opacity = "0";
        document.body.appendChild(area);
        area.select();
        document.execCommand("copy");
        document.body.removeChild(area);
      }
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      setCopied(false);
    }
  };

  return (
    <Show when={props.job}>
      {(job) => (
        <aside class="absolute top-0 right-0 bottom-0 z-30 w-[760px] max-w-[78vw] min-w-[460px] border-l border-mk-separator bg-mk-bg/96 backdrop-blur-sm flex flex-col min-h-0 shadow-2xl animate-drawer-in">
          <div class="sticky top-0 z-10 px-5 py-3 border-b border-mk-separator/80 bg-mk-bg/94 backdrop-blur-sm">
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="text-[10px] font-semibold uppercase tracking-widest text-mk-tertiary">Job Snapshot</p>
                <h3 class="mt-1 text-[19px] font-semibold text-mk-text leading-snug break-words">{job().title}</h3>
              </div>
              <button
                class="shrink-0 p-1 rounded-md text-mk-tertiary hover:text-mk-text hover:bg-mk-fill transition-all"
                aria-label="Close details panel"
                onClick={props.onClose}
              >
                <X class="w-4 h-4" />
              </button>
            </div>
            <div class="mt-2.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[13px]">
              <span class="inline-flex items-center gap-1 text-mk-secondary">
                <CalendarDays class="w-3.5 h-3.5 text-mk-tertiary" />
                {formatPosted(job().posted_at)}
              </span>
              <span class="inline-flex items-center gap-1 text-mk-secondary">
                <Wallet class="w-3.5 h-3.5 text-mk-tertiary" />
                {job().pay || "-"}
              </span>
              <span class="inline-flex items-center gap-1 text-mk-secondary">
                <Tag class="w-3.5 h-3.5 text-mk-tertiary" />
                {job().keyword || "-"}
              </span>
            </div>
          </div>

          <div class="flex-1 overflow-auto p-5 space-y-4">
            <div class="flex items-center gap-3 pb-3 border-b border-mk-separator/70">
              <Show
                when={(crawled()?.company_logo_url || job().company_logo_url) && /^https?:\/\//.test(crawled()?.company_logo_url || job().company_logo_url || "")}
                fallback={
                  <div class="w-12 h-12 rounded-md bg-mk-fill border border-mk-separator/80 flex items-center justify-center text-mk-secondary">
                    <Building2 class="w-5 h-5" />
                  </div>
                }
              >
                <img
                  src={crawled()?.company_logo_url || job().company_logo_url}
                  alt={crawled()?.company || job().company || "Company logo"}
                  class="w-12 h-12 rounded-md object-cover border border-mk-separator/80 bg-mk-fill"
                  referrerPolicy="no-referrer"
                />
              </Show>
              <div class="min-w-0">
                <p class="text-[15px] font-semibold text-mk-text truncate">{crawled()?.company || job().company || "Unknown company"}</p>
                <p class="text-[13px] text-mk-tertiary">
                  {crawled()?.poster_name ? `Posted by ${crawled()?.poster_name}` : job().source}
                </p>
              </div>
            </div>

            <div>
              <p class="text-[13px] font-semibold uppercase tracking-widest text-mk-tertiary mb-2">Description</p>
              <div class="max-w-[68ch] pr-2">
                <Show
                  when={!loadingDetails()}
                  fallback={<p class="text-[15px] leading-8 text-mk-secondary">Loading full job description...</p>}
                >
                  {() => {
                    const html = buildDescriptionHtml(crawled(), job().summary);
                    return (
                      <div
                        class="job-description-content text-[15px] text-mk-secondary [overflow-wrap:anywhere]"
                        innerHTML={html || "<p>No description available.</p>"}
                      />
                    );
                  }}
                </Show>
              </div>
            </div>
          </div>

          <div class="px-5 py-3 border-t border-mk-separator flex items-center justify-between">
            <button
              class="text-[14px] font-medium text-mk-tertiary hover:text-mk-text transition-colors"
              onClick={props.onClose}
            >
              Close
            </button>
            <div class="flex items-center gap-3">
              <button
                class="inline-flex items-center gap-1 text-[13px] font-medium text-mk-secondary hover:text-mk-text transition-colors"
                onClick={copyJobContext}
              >
                <Copy class="w-3.5 h-3.5" />
                {copied() ? "Copied" : "Copy context"}
              </button>
              <button
                class="inline-flex items-center gap-1 text-[14px] font-semibold text-mk-cyan hover:opacity-80 transition-opacity"
                onClick={() => props.onOpenUrl(job().url)}
              >
                Open full listing
                <ExternalLink class="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
        </aside>
      )}
    </Show>
  );
}
