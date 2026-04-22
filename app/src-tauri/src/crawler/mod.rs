use crate::db::{Database, Job};
use chrono::Utc;
use reqwest::Client;
use reqwest::StatusCode;
use reqwest::Url;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::Channel;

const BASE_URL: &str = "https://www.onlinejobs.ph/jobseekers/jobsearch";
const SITE_BASE: &str = "https://www.onlinejobs.ph";
const BRUNTWORK_SEARCH_URL: &str = "https://www.bruntworkcareers.co/search";
const BRUNTWORK_SITE_BASE: &str = "https://www.bruntworkcareers.co";
const CRAWL_DELAY: Duration = Duration::from_secs(5);
const MAX_PAGES: usize = 5;
const FETCH_MAX_ATTEMPTS: usize = 3;
const FETCH_RETRY_BASE_DELAY: Duration = Duration::from_millis(700);
const SCRAPLING_BASE_URL_ENV: &str = "EZER_SCRAPLING_BASE_URL";
const ALLOWED_JOB_HOSTS: &[&str] = &[
    "onlinejobs.ph",
    "www.onlinejobs.ph",
    "bruntworkcareers.co",
    "www.bruntworkcareers.co",
];

pub mod webview_scraper;

pub struct Crawler {
    client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDetailsPayload {
    pub company: String,
    pub poster_name: String,
    pub company_logo_url: String,
    pub description: String,
    pub description_html: String,
    pub job_type: String,
    pub posted_at: String,
}

#[derive(Debug)]
struct FetchAttemptError {
    message: String,
    retryable: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ScraplingSearchRequest {
    url: String,
    keyword: String,
    html: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScraplingSearchResponse {
    jobs: Vec<ScraplingJob>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScraplingJob {
    source_id: Option<String>,
    title: Option<String>,
    company: Option<String>,
    company_logo_url: Option<String>,
    pay: Option<String>,
    posted_at: Option<String>,
    url: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ScraplingDetailsRequest {
    url: String,
    html: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScraplingDetailsResponse {
    company: Option<String>,
    poster_name: Option<String>,
    company_logo_url: Option<String>,
    description: Option<String>,
    description_html: Option<String>,
    posted_at: Option<String>,
}

impl Crawler {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .user_agent("ezerpath/1.0 (+personal research crawler)")
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { client })
    }

    pub async fn crawl_keyword(
        &self,
        keyword: &str,
        db: &Arc<Database>,
        days: u32,
        run_id: i64,
        on_progress: Option<&Channel<ScanProgress>>,
    ) -> Result<CrawlStats, String> {
        let mut stats = CrawlStats { keyword: keyword.to_string(), found: 0, new: 0, pages: 0 };
        let encoded = urlencoding::encode(keyword);

        for page_num in 0..MAX_PAGES {
            let offset = page_num * 30;
            let url = if offset == 0 {
                format!("{}?jobkeyword={}&dateposted={}", BASE_URL, encoded, days)
            } else {
                format!("{}/{}?jobkeyword={}&dateposted={}", BASE_URL, offset, encoded, days)
            };

            let html = self.fetch_with_retry(&url).await?;
            let mut parse_error: Option<String> = None;
            let mut jobs = match parse_search_page(&html, keyword) {
                Ok(rows) => rows,
                Err(err) => {
                    parse_error = Some(err);
                    Vec::new()
                }
            };

            if jobs.is_empty() {
                if let Some(fallback_jobs) = self
                    .try_scrapling_search_fallback(&url, keyword, Some(&html))
                    .await
                {
                    jobs = fallback_jobs;
                }
            }

            if jobs.is_empty() {
                if let Some(err) = parse_error {
                    return Err(format!("Failed to parse search page: {err}"));
                }
                break;
            }

            // Client-side date guard: the site's dateposted filter is not always
            // precise. Drop any job whose posted_at parses to older than requested.
            let cutoff = days as i64;
            let now_dt = Utc::now();
            jobs.retain(|job| {
                match posted_at_days_ago(&job.posted_at, &now_dt) {
                    Some(d) => d <= cutoff,
                    None => true, // unparseable posted_at → keep conservatively
                }
            });
            if jobs.is_empty() {
                // All jobs on this page are out of range; later pages will be older.
                break;
            }

            stats.pages += 1;
            for job in &jobs {
                stats.found += 1;
                if db.insert_job(job, run_id).map_err(|e| e.to_string())? {
                    stats.new += 1;
                }
            }

            emit_progress(
                on_progress,
                ScanProgress::Page {
                    keyword: keyword.to_string(),
                    page: page_num + 1,
                    found: stats.found,
                },
            );

            tokio::time::sleep(CRAWL_DELAY).await;
        }

        Ok(stats)
    }

    async fn fetch_once(&self, url: &str) -> Result<String, FetchAttemptError> {
        let resp = self.client.get(url).send().await.map_err(|e| FetchAttemptError {
            message: e.to_string(),
            retryable: e.is_timeout() || e.is_connect() || e.is_request(),
        })?;
        if !resp.status().is_success() {
            return Err(FetchAttemptError {
                message: format!("HTTP {}", resp.status()),
                retryable: is_retryable_status(resp.status()),
            });
        }
        resp.text().await.map_err(|e| FetchAttemptError {
            message: e.to_string(),
            retryable: true,
        })
    }

    /// Fetch the same URL as an RSC payload (Next.js App Router). Returns the
    /// raw response text which contains the serialized RSC stream (the same
    /// format as the `self.__next_f.push` chunks, but without HTML wrapping
    /// and including the full server-rendered data).
    async fn fetch_rsc_payload(&self, url: &str) -> Option<String> {
        let resp = self.client
            .get(url)
            .header("RSC", "1")
            .header("Accept", "text/x-component")
            .header("Next-Router-State-Tree", "%5B%22%22%2C%7B%7D%2C%22%22%2Cnull%2Cnull%2Ctrue%5D")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.text().await.ok()
    }

    async fn fetch_with_retry(&self, url: &str) -> Result<String, String> {
        let mut last_err = String::from("unknown fetch error");
        for attempt in 1..=FETCH_MAX_ATTEMPTS {
            match self.fetch_once(url).await {
                Ok(text) => return Ok(text),
                Err(err) => {
                    last_err = err.message;
                    if !err.retryable || attempt == FETCH_MAX_ATTEMPTS {
                        break;
                    }
                    let factor = 1_u32 << ((attempt - 1) as u32);
                    tokio::time::sleep(FETCH_RETRY_BASE_DELAY * factor).await;
                }
            }
        }
        Err(last_err)
    }

    fn scrapling_base_url() -> Option<String> {
        // Scrapling is always-on: defaults to the same host/port as the bundled
        // ai_service (127.0.0.1:8765). Override with EZER_SCRAPLING_BASE_URL.
        // If the service isn't running, HTTP calls will simply fail and the
        // crawler falls back to its built-in parsers — no config needed.
        let base = std::env::var(SCRAPLING_BASE_URL_ENV)
            .unwrap_or_else(|_| "http://127.0.0.1:8765".to_string());
        let trimmed = base.trim().trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    async fn try_scrapling_search_fallback(
        &self,
        url: &str,
        keyword: &str,
        html: Option<&str>,
    ) -> Option<Vec<Job>> {
        let base = Self::scrapling_base_url()?;
        let endpoint = format!("{base}/extract-search");
        let payload = ScraplingSearchRequest {
            url: url.to_string(),
            keyword: keyword.to_string(),
            html: html.map(|s| s.to_string()),
        };
        let resp = self.client.post(endpoint).json(&payload).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let data = resp.json::<ScraplingSearchResponse>().await.ok()?;
        let now = Utc::now().to_rfc3339();
        let mut jobs = Vec::new();
        for row in data.jobs {
            let title = row.title.unwrap_or_default();
            let url = row.url.unwrap_or_default();
            if title.trim().is_empty() || url.trim().is_empty() || !is_allowed_job_url(&url) {
                continue;
            }
            let source_id = row.source_id.unwrap_or_else(|| {
                url.rsplit('/').next().unwrap_or_default().to_string()
            });
            let summary = row.summary.map(|s| normalize_text(&s)).unwrap_or_default();
            let job_type = infer_job_type(&title, &summary);
            jobs.push(Job {
                id: None,
                source: "onlinejobs".to_string(),
                source_id,
                title: normalize_text(&title),
                company: row.company.map(|s| normalize_text(&s)).unwrap_or_default(),
                company_logo_url: row
                    .company_logo_url
                    .map(|s| normalize_asset_url(&s))
                    .unwrap_or_default(),
                pay: row.pay.map(|s| normalize_text(&s)).unwrap_or_default(),
                posted_at: row.posted_at.map(|s| normalize_text(&s)).unwrap_or_default(),
                url,
                summary,
                keyword: keyword.to_string(),
                scraped_at: now.clone(),
                is_new: true,
                watchlisted: false,
                run_id: None,
                salary_min: None,
                salary_max: None,
                salary_currency: String::new(),
                salary_period: String::new(),
                applied: false,
                job_type,
            });
        }
        if jobs.is_empty() {
            None
        } else {
            Some(jobs)
        }
    }

    async fn try_scrapling_details_fallback(&self, url: &str, html: Option<&str>) -> Option<JobDetailsPayload> {
        let base = Self::scrapling_base_url()?;
        let endpoint = format!("{base}/extract-details");
        let payload = ScraplingDetailsRequest {
            url: url.to_string(),
            html: html.map(|s| s.to_string()),
        };
        let resp = self.client.post(endpoint).json(&payload).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let data = resp.json::<ScraplingDetailsResponse>().await.ok()?;
        let description = data.description.map(|s| normalize_text(&s)).unwrap_or_default();
        let description_html = data.description_html.unwrap_or_default();
        let text_for_inference = if description.is_empty() { &description_html } else { &description };
        let job_type = infer_job_type("", text_for_inference);
        Some(JobDetailsPayload {
            company: data.company.map(|s| normalize_text(&s)).unwrap_or_default(),
            poster_name: data.poster_name.map(|s| normalize_text(&s)).unwrap_or_default(),
            company_logo_url: data
                .company_logo_url
                .map(|s| normalize_asset_url(&s))
                .unwrap_or_default(),
            description,
            description_html,
            job_type,
            posted_at: data.posted_at.unwrap_or_default(),
        })
    }

    pub async fn fetch_job_details(&self, url: &str) -> Result<JobDetailsPayload, String> {
        let parsed_url = parse_allowed_job_url(url)?;
        let html = self.fetch_with_retry(url).await?;
        if is_bruntwork_job_url(&parsed_url) {
            // Bruntwork uses Next.js App Router (RSC streaming); a plain HTTP fetch
            // does not contain rendered content. Try scrapling (headless browser) first.
            if let Some(scrapled) = self.try_scrapling_details_fallback(url, None).await {
                // Reject if scrapling returned RSC streaming garbage instead of real content
                let has_garbage = is_rsc_garbage(&scrapled.description)
                    || is_rsc_garbage(&scrapled.description_html);
                if is_meaningful_job_details(&scrapled) && !has_garbage {
                    let mut result = scrapled;
                    if result.posted_at.is_empty() {
                        result.posted_at = extract_bruntwork_published_date(&html);
                    }
                    return Ok(result);
                }
            }

            // Try parsing the static HTML first (cheap).
            let mut payload = parse_bruntwork_job_details(&html)?;

            // If description is still empty, try fetching the RSC payload via the
            // `RSC: 1` header. Next.js App Router returns the full server-rendered
            // data (including descriptions) in this format.
            if payload.description.is_empty() && payload.description_html.is_empty() {
                if let Some(rsc) = self.fetch_rsc_payload(url).await {
                    if let Some((desc, desc_html)) = try_parse_rsc_payload_description(&rsc) {
                        payload.description = desc;
                        payload.description_html = desc_html;
                        let text = if payload.description.is_empty() {
                            &payload.description_html
                        } else {
                            &payload.description
                        };
                        if payload.job_type.is_empty() {
                            payload.job_type = infer_job_type("", text);
                        }
                    }
                }
            }
            return Ok(payload);
        }
        let parsed = parse_job_details(&html)?;
        if is_meaningful_job_details(&parsed) {
            return Ok(parsed);
        }
        if let Some(fallback) = self.try_scrapling_details_fallback(url, Some(&html)).await {
            if is_meaningful_job_details(&fallback) {
                return Ok(fallback);
            }
        }
        Ok(parsed)
    }

    /// Scrapling fallback for Bruntwork: asks the scrapling service to render
    /// the Bruntwork search page with a headless browser and return job data.
    async fn try_scrapling_bruntwork_fallback(&self) -> Option<Vec<Job>> {
        let base = Self::scrapling_base_url()?;
        let endpoint = format!("{base}/extract-search");
        // Send without html so scrapling fetches fresh with a real browser.
        let payload = ScraplingSearchRequest {
            url: BRUNTWORK_SEARCH_URL.to_string(),
            keyword: String::new(),
            html: None,
        };
        let resp = self.client.post(endpoint).json(&payload).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let data = resp.json::<ScraplingSearchResponse>().await.ok()?;
        let now = Utc::now().to_rfc3339();
        let mut jobs = Vec::new();
        for row in data.jobs {
            let url = row.url.unwrap_or_default();
            if !url.contains("bruntworkcareers.co/jobs/") { continue; }
            let title_raw = row.title.unwrap_or_default();
            if title_raw.trim().is_empty() { continue; }
            let source_id = url
                .split("/jobs/").nth(1)
                .and_then(|s| s.split('/').next())
                .unwrap_or_default()
                .to_string();
            if source_id.is_empty() { continue; }
            let (title, mut job_type) = split_bruntwork_title_type(&title_raw);
            if job_type.is_empty() {
                job_type = infer_job_type(&title, &row.summary.as_deref().unwrap_or(""));
            }
            jobs.push(Job {
                id: None,
                source: "bruntwork".to_string(),
                source_id,
                title,
                company: "BruntWork".to_string(),
                company_logo_url: String::new(),
                pay: String::new(),
                posted_at: row.posted_at.map(|s| normalize_text(&s)).unwrap_or_default(),
                url,
                summary: row.summary.map(|s| normalize_text(&s)).unwrap_or_default(),
                keyword: String::new(),
                scraped_at: now.clone(),
                is_new: true,
                watchlisted: false,
                run_id: None,
                salary_min: None,
                salary_max: None,
                salary_currency: String::new(),
                salary_period: String::new(),
                applied: false,
                job_type,
            });
        }
        if jobs.is_empty() { None } else { Some(jobs) }
    }

    /// Fetch all Bruntwork listings once and store those matching any keyword.
    /// Non-fatal: if the fetch fails, an empty stats list is returned (onlinejobs
    /// results are already committed and should not be rolled back).
    pub async fn crawl_bruntwork(
        &self,
        keywords: &[String],
        db: &Arc<Database>,
        run_id: i64,
        on_progress: Option<&Channel<ScanProgress>>,
    ) -> Vec<CrawlStats> {
        let html = self.fetch_with_retry(BRUNTWORK_SEARCH_URL).await.ok();
        let all_jobs = match html.as_deref().map(parse_bruntwork_search) {
            Some(Ok(jobs)) if !jobs.is_empty() => jobs,
            _ => {
                // Plain HTTP returned nothing (JS-rendered page or blocked).
                // Try scrapling which uses a real headless browser.
                match self.try_scrapling_bruntwork_fallback().await {
                    Some(jobs) if !jobs.is_empty() => jobs,
                    _ => {
                        let reason = html.as_deref()
                            .map(|_| "parse returned 0 jobs")
                            .unwrap_or("fetch failed");
                        eprintln!("[bruntwork] {reason}, scrapling also empty or disabled");
                        return Vec::new();
                    }
                }
            }
        };

        let mut all_stats: Vec<CrawlStats> = Vec::new();
        for keyword in keywords {
            let kw_lower = keyword.to_lowercase();
            let matching: Vec<Job> = all_jobs
                .iter()
                .filter(|j| {
                    let t = j.title.to_lowercase();
                    // Full keyword match OR any significant word (>3 chars) matches
                    t.contains(&kw_lower)
                        || kw_lower
                            .split_whitespace()
                            .filter(|w| w.len() > 3)
                            .any(|w| t.contains(w))
                })
                .map(|j| {
                    let mut j = j.clone();
                    j.keyword = keyword.clone();
                    j
                })
                .collect();

            let mut stats = CrawlStats { keyword: keyword.clone(), found: 0, new: 0, pages: 1 };
            for job in &matching {
                stats.found += 1;
                match db.insert_job(job, run_id) {
                    Ok(true) => stats.new += 1,
                    Ok(false) => {}
                    Err(err) => eprintln!("[bruntwork] insert_job error: {err}"),
                }
            }
            emit_progress(
                on_progress,
                ScanProgress::BruntworkKeyword {
                    keyword: keyword.clone(),
                    found: stats.found,
                    new: stats.new,
                },
            );
            all_stats.push(stats);
        }
        all_stats
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CrawlStats {
    pub keyword: String,
    pub found: usize,
    pub new: usize,
    pub pages: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScanProgress {
    Started {
        run_id: i64,
        total_keywords: usize,
        keywords: Vec<String>,
    },
    KeywordStarted {
        keyword: String,
        index: usize,
        total: usize,
    },
    Page {
        keyword: String,
        page: usize,
        found: usize,
    },
    KeywordCompleted {
        keyword: String,
        found: usize,
        new: usize,
        pages: usize,
    },
    Completed {
        run_id: i64,
        total_found: i64,
        total_new: i64,
    },
    Failed {
        run_id: i64,
        error: String,
    },
    BruntworkKeyword {
        keyword: String,
        found: usize,
        new: usize,
    },
}

fn emit_progress(channel: Option<&Channel<ScanProgress>>, payload: ScanProgress) {
    if let Some(ch) = channel {
        // Best-effort: a closed channel just means the frontend went away.
        let _ = ch.send(payload);
    }
}

fn parse_search_page(html: &str, keyword: &str) -> Result<Vec<Job>, String> {
    let doc = Html::parse_document(html);
    let card_sel = Selector::parse(".jobpost-cat-box").map_err(|e| e.to_string())?;
    let title_sel = Selector::parse("h4, h3, .job-title, [class*='title']").map_err(|e| e.to_string())?;
    let date_sel = Selector::parse("p.fs-13 em").map_err(|e| e.to_string())?;
    let pay_sel  = Selector::parse("dl.no-gutters dd, .rate, [class*='salary'], [class*='pay']").map_err(|e| e.to_string())?;
    let desc_sel = Selector::parse(".desc, [class*='description'], [class*='summary']").map_err(|e| e.to_string())?;
    let logo_sel = Selector::parse(".jobpost-cat-box-logo").map_err(|e| e.to_string())?;
    let logo_img_sel = Selector::parse(".jobpost-cat-box-logo img").map_err(|e| e.to_string())?;
    let link_sel = Selector::parse("a").map_err(|e| e.to_string())?;

    let now = Utc::now().to_rfc3339();
    let mut jobs = Vec::new();

    for card in doc.select(&card_sel) {
        let title = card.select(&title_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let posted_at = card.select(&date_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default()
            .replace("Posted on ", "")
            .replace("posted on ", "");

        let company = card.select(&logo_img_sel)
            .next()
            .and_then(|e| e.value().attr("alt"))
            .or_else(|| card.select(&logo_sel).next().and_then(|e| e.value().attr("alt")))
            .unwrap_or("")
            .to_string();

        let company_logo_url = card
            .select(&logo_img_sel)
            .next()
            .and_then(|e| {
                e.value()
                    .attr("src")
                    .or_else(|| e.value().attr("data-src"))
                    .or_else(|| e.value().attr("srcset").and_then(|s| s.split(',').next().and_then(|p| p.split_whitespace().next())))
            })
            .map(normalize_asset_url)
            .unwrap_or_default();

        let pay = card.select(&pay_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // Try CSS selector first; fall back to parsing the card's inner HTML
        // when the scraper crate's tree construction skips the .desc element.
        let summary = card.select(&desc_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                let inner = card.inner_html();
                if let Some(start) = inner.find("class=\"desc").or_else(|| inner.find("class='desc")) {
                    if let Some(gt) = inner[start..].find('>') {
                        let after = start + gt + 1;
                        if let Some(end) = inner[after..].find("</div>") {
                            let raw = &inner[after..after + end];
                            let stripped = raw
                                .replace("<br>", " ")
                                .replace("<br/>", " ")
                                .replace("<br />", " ");
                            // Strip remaining HTML tags
                            let mut out = String::new();
                            let mut in_tag = false;
                            for ch in stripped.chars() {
                                match ch {
                                    '<' => in_tag = true,
                                    '>' => in_tag = false,
                                    _ if !in_tag => out.push(ch),
                                    _ => {}
                                }
                            }
                            return out.split_whitespace().collect::<Vec<_>>().join(" ");
                        }
                    }
                }
                String::new()
            });
        let summary = if summary.chars().count() > 500 {
            let mut s = String::new();
            for ch in summary.chars().take(497) { s.push(ch); }
            s.push_str("...");
            s
        } else {
            summary
        };

        // Find job link
        let mut url = String::new();
        let mut source_id = String::new();
        for link in card.select(&link_sel) {
            if let Some(href) = link.value().attr("href") {
                if href.contains("/jobseekers/job/") {
                    url = if href.starts_with("http") {
                        href.to_string()
                    } else {
                        format!("https://www.onlinejobs.ph{}", href)
                    };
                    // Extract numeric ID from end of URL
                    source_id = href.rsplit('/').next()
                        .and_then(|s| {
                            // ID might be at end after a dash: job-title-123456
                            s.rsplit('-').next()
                                .filter(|id| id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty())
                                .or(Some(s))
                        })
                        .unwrap_or("")
                        .to_string();
                    break;
                }
            }
        }

        if url.is_empty() || !is_allowed_job_url(&url) {
            continue;
        }

        let job_type = infer_job_type(&title, &summary);
        jobs.push(Job {
            id: None,
            source: "onlinejobs".to_string(),
            source_id,
            title,
            company,
            company_logo_url,
            pay,
            posted_at,
            url,
            summary,
            keyword: keyword.to_string(),
            scraped_at: now.clone(),
            is_new: true,
            watchlisted: false,
            run_id: None,
            salary_min: None,
            salary_max: None,
            salary_currency: String::new(),
            salary_period: String::new(),
            applied: false,
            job_type,
        });
    }

    Ok(jobs)
}

fn parse_job_details(html: &str) -> Result<JobDetailsPayload, String> {
    let doc = Html::parse_document(html);
    let script_sel = Selector::parse("script[type='application/ld+json']").map_err(|e| e.to_string())?;
    let desc_sel = Selector::parse(
        ".job-description, #job-description, .jobpost-description, .description, .desc, [class*='description']",
    )
    .map_err(|e| e.to_string())?;
    let company_sel = Selector::parse(
        ".company-name, [class*='company-name'], [class*='job-company'], [class*='employer-name']",
    )
    .map_err(|e| e.to_string())?;
    let poster_sel = Selector::parse(
        ".job-poster-name, [class*='poster-name'], [class*='hired-by'], [class*='employer-name'], [class*='client-name']",
    )
    .map_err(|e| e.to_string())?;
    let logo_sel = Selector::parse(
        ".company img, [class*='company'] img, [class*='employer'] img, [class*='client'] img",
    )
    .map_err(|e| e.to_string())?;

    let mut company = String::new();
    let mut poster_name = String::new();
    let mut company_logo_url = String::new();
    let mut description = String::new();
    let mut description_html = String::new();
    let mut job_type = String::new();

    for script in doc.select(&script_sel) {
        let raw = script.text().collect::<String>();
        if raw.trim().is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            extract_jsonld_fields(
                &value,
                &mut company,
                &mut poster_name,
                &mut company_logo_url,
                &mut description,
            );
            if job_type.is_empty() {
                if let Some(et) = find_first_key_string(&value, "employmentType") {
                    job_type = map_employment_type(&et);
                }
            }
        }
    }

    if company.is_empty() {
        company = extract_best_text(&doc, &company_sel);
    }
    if poster_name.is_empty() {
        poster_name = extract_best_text(&doc, &poster_sel);
    }
    if company_logo_url.is_empty() {
        company_logo_url = doc
            .select(&logo_sel)
            .next()
            .and_then(|e| {
                e.value()
                    .attr("src")
                    .or_else(|| e.value().attr("data-src"))
                    .or_else(|| {
                        e.value()
                            .attr("srcset")
                            .and_then(|s| s.split(',').next().and_then(|p| p.split_whitespace().next()))
                    })
            })
            .map(normalize_asset_url)
            .unwrap_or_default();
    }
    if description.is_empty() {
        description = extract_longest_text(&doc, &desc_sel);
    }
    if description_html.is_empty() {
        description_html = doc
            .select(&desc_sel)
            .next()
            .map(|e| e.inner_html())
            .unwrap_or_default();
    }

    if job_type.is_empty() {
        job_type = infer_job_type("", &description);
    }

    Ok(JobDetailsPayload {
        company,
        poster_name,
        company_logo_url,
        description,
        description_html,
        job_type,
        posted_at: String::new(),
    })
}

fn extract_jsonld_fields(
    value: &Value,
    company: &mut String,
    poster_name: &mut String,
    company_logo_url: &mut String,
    description: &mut String,
) {
    if description.is_empty() {
        if let Some(desc) = find_first_key_string(value, "description") {
            *description = normalize_text(&desc);
        }
    }

    if company.is_empty() || company_logo_url.is_empty() {
        if let Some(hiring_org) = find_first_key(value, "hiringOrganization") {
            if company.is_empty() {
                if let Some(name) = find_first_key_string(hiring_org, "name") {
                    *company = normalize_text(&name);
                }
            }
            if company_logo_url.is_empty() {
                if let Some(logo) = find_first_key_string(hiring_org, "logo") {
                    *company_logo_url = normalize_asset_url(&logo);
                }
            }
        }
    }

    if poster_name.is_empty() {
        if let Some(author) = find_first_key(value, "author") {
            if let Some(name) = find_first_key_string(author, "name") {
                *poster_name = normalize_text(&name);
            }
        } else if let Some(posted_by) = find_first_key(value, "postedBy") {
            if let Some(name) = find_first_key_string(posted_by, "name") {
                *poster_name = normalize_text(&name);
            }
        }
    }
}

fn find_first_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            for v in map.values() {
                if let Some(found) = find_first_key(v, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for v in arr {
                if let Some(found) = find_first_key(v, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn find_first_key_string(value: &Value, key: &str) -> Option<String> {
    let found = find_first_key(value, key)?;
    match found {
        Value::String(s) => Some(s.to_string()),
        Value::Object(map) => map.get("@id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        _ => None,
    }
}

fn map_employment_type(raw: &str) -> String {
    match raw.to_uppercase().replace('-', "_").as_str() {
        "FULL_TIME" | "FULLTIME" => "Full-Time".to_string(),
        "PART_TIME" | "PARTTIME" => "Part-Time".to_string(),
        "CONTRACTOR" | "CONTRACT" | "FREELANCE" => "Contract".to_string(),
        _ => String::new(),
    }
}

fn infer_job_type(title: &str, text: &str) -> String {
    let combined = format!("{} {}", title, text).to_lowercase();
    let full = combined.contains("full-time")
        || combined.contains("full time")
        || combined.contains("fulltime");
    let part = combined.contains("part-time")
        || combined.contains("part time")
        || combined.contains("parttime");
    let hours = extract_weekly_hours(&combined);
    match (full, part) {
        (true, _) => match hours {
            Some(h) => format!("Full-Time ({} hrs/wk)", h),
            None => "Full-Time".to_string(),
        },
        (false, true) => match hours {
            Some(h) => format!("Part-Time ({} hrs/wk)", h),
            None => "Part-Time".to_string(),
        },
        (false, false) => match hours {
            Some(h) if h >= 35 => format!("Full-Time ({} hrs/wk)", h),
            Some(h) => format!("Part-Time ({} hrs/wk)", h),
            None => String::new(),
        },
    }
}

fn extract_weekly_hours(text: &str) -> Option<u32> {
    let mut chars = text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if ch.is_ascii_digit() {
            let start = i;
            let mut end = i + ch.len_utf8();
            while let Some(&(j, c)) = chars.peek() {
                if c.is_ascii_digit() {
                    end = j + c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            if let Ok(n) = text[start..end].parse::<u32>() {
                if n >= 1 && n <= 80 {
                    let rest = text[end..].trim_start_matches(|c: char| c == ' ' || c == '-');
                    if rest.starts_with("hours/week")
                        || rest.starts_with("hours per week")
                        || rest.starts_with("hours a week")
                        || rest.starts_with("hours weekly")
                        || rest.starts_with("hrs/week")
                        || rest.starts_with("hrs per week")
                        || rest.starts_with("hrs a week")
                        || rest.starts_with("hrs weekly")
                        || rest.starts_with("h/week")
                        || rest.starts_with("h/wk")
                        || rest.starts_with("hours/wk")
                        || rest.starts_with("hrs/wk")
                    {
                        return Some(n);
                    }
                    if rest.starts_with("hour") || rest.starts_with("hr") {
                        let after = rest.trim_start_matches(|c: char| c.is_alphabetic() || c == '-' || c == '/');
                        let after = after.trim_start_matches(' ');
                        if after.starts_with("week") || after.starts_with("wk") || after.starts_with("per") {
                            return Some(n);
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_best_text(doc: &Html, selector: &Selector) -> String {
    doc.select(selector)
        .map(|e| normalize_text(&e.text().collect::<String>()))
        .find(|t| !t.is_empty())
        .unwrap_or_default()
}

fn extract_longest_text(doc: &Html, selector: &Selector) -> String {
    doc.select(selector)
        .map(|e| normalize_text(&e.text().collect::<String>()))
        .filter(|t| t.len() >= 60)
        .max_by_key(|t| t.len())
        .unwrap_or_default()
}

fn normalize_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::FORBIDDEN
        || status == StatusCode::CONFLICT
        || status == StatusCode::TOO_EARLY
        || status.is_server_error()
}

pub(crate) fn is_meaningful_job_details(payload: &JobDetailsPayload) -> bool {
    !payload.description.trim().is_empty()
        || !payload.description_html.trim().is_empty()
        || !payload.company.trim().is_empty()
}

pub(crate) fn parse_allowed_job_url(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|_| "Unsupported job URL".to_string())?;
    if parsed.scheme() != "https" {
        return Err("Unsupported job URL".to_string());
    }
    if !matches!(parsed.host_str(), Some(host) if ALLOWED_JOB_HOSTS.contains(&host)) {
        return Err("Unsupported job URL".to_string());
    }
    Ok(parsed)
}

pub(crate) fn is_allowed_job_url(url: &str) -> bool {
    parse_allowed_job_url(url).is_ok()
}

pub(crate) fn is_bruntwork_job_url(parsed: &Url) -> bool {
    matches!(
        parsed.host_str(),
        Some("bruntworkcareers.co") | Some("www.bruntworkcareers.co")
    )
}

fn normalize_asset_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = Url::parse(trimmed) {
        return parsed.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return format!("https://{rest}");
    }
    if let Ok(base) = Url::parse(SITE_BASE) {
        if let Ok(joined) = base.join(trimmed) {
            return joined.to_string();
        }
    }
    String::new()
}

/// Parse a human-readable `posted_at` string as returned by OnlineJobs.ph
/// and return how many days ago the job was posted.
///
/// Handles:
/// - Same-day: `"today"`, `"just now"`, `"X hours ago"`, `"X minutes ago"`
/// - Relative:  `"yesterday"`, `"N day(s) ago"`, `"N week(s) ago"`,
///              `"N month(s) ago"`, `"N year(s) ago"`
/// - ISO-like:  `"2026-04-16 05:20:50"`, `"2026-04-16"` (the actual format from onlinejobs.ph)
/// - Absolute:  `"April 15, 2026"`, `"Apr 5, 2026"` (Month D(D), YYYY)
///
/// Returns `None` when the format is not recognised; callers should treat
/// an unknown date as "keep" (conservative).
pub(crate) fn posted_at_days_ago(posted_at: &str, now: &chrono::DateTime<Utc>) -> Option<i64> {
    let s = posted_at.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }

    // Same-day indicators
    if s == "today"
        || s == "just now"
        || s.contains("hours ago")
        || s.contains("hour ago")
        || s.contains("minutes ago")
        || s.contains("minute ago")
        || s.contains("seconds ago")
    {
        return Some(0);
    }
    if s.contains("yesterday") {
        return Some(1);
    }

    // Relative: "N day(s) / week(s) / month(s) / year(s) ago"
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() >= 3 && words.last() == Some(&"ago") {
        if let Ok(n) = words[0].parse::<i64>() {
            let unit = words[1];
            let days = if unit.starts_with("day") {
                n
            } else if unit.starts_with("week") {
                n * 7
            } else if unit.starts_with("month") {
                n * 30
            } else if unit.starts_with("year") {
                n * 365
            } else {
                return None;
            };
            return Some(days);
        }
    }

    // ISO-like: "2026-04-16 05:20:50" (the actual format from onlinejobs.ph)
    if let Ok(naive_dt) = chrono::NaiveDateTime::parse_from_str(posted_at.trim(), "%Y-%m-%d %H:%M:%S") {
        let diff = now.date_naive().signed_duration_since(naive_dt.date()).num_days();
        return Some(diff.max(0));
    }
    // Also try date-only ISO: "2026-04-16"
    if let Ok(naive_d) = chrono::NaiveDate::parse_from_str(posted_at.trim(), "%Y-%m-%d") {
        let diff = now.date_naive().signed_duration_since(naive_d).num_days();
        return Some(diff.max(0));
    }

    // Absolute: "Month D(D), YYYY"  e.g. "April 5, 2026" or "April 15, 2026"
    let parts: Vec<&str> = posted_at.trim().split_whitespace().collect();
    if parts.len() == 3 {
        let month_str = parts[0].to_lowercase();
        let day_str = parts[1].trim_end_matches(',');
        let year_str = parts[2];
        if let (Ok(day), Ok(year)) = (day_str.parse::<u32>(), year_str.parse::<i32>()) {
            let month: u32 = match month_str.as_str() {
                "january"   | "jan"  => 1,
                "february"  | "feb"  => 2,
                "march"     | "mar"  => 3,
                "april"     | "apr"  => 4,
                "may"                => 5,
                "june"      | "jun"  => 6,
                "july"      | "jul"  => 7,
                "august"    | "aug"  => 8,
                "september" | "sep"
                            | "sept" => 9,
                "october"   | "oct"  => 10,
                "november"  | "nov"  => 11,
                "december"  | "dec"  => 12,
                _                    => return None,
            };
            if let Some(naive) = chrono::NaiveDate::from_ymd_opt(year, month, day) {
                let diff = now.date_naive().signed_duration_since(naive).num_days();
                return Some(diff.max(0));
            }
        }
    }

    None
}

// ── Bruntwork Careers ────────────────────────────────────────────────────────

fn parse_bruntwork_search(html: &str) -> Result<Vec<Job>, String> {
    let now = Utc::now().to_rfc3339();
    if let Some(jobs) = try_parse_bruntwork_next_data(html, &now) {
        if !jobs.is_empty() {
            return Ok(jobs);
        }
    }
    parse_bruntwork_search_html(html, &now)
}

fn try_parse_bruntwork_next_data(html: &str, now: &str) -> Option<Vec<Job>> {
    let doc = Html::parse_document(html);
    let script_sel = Selector::parse("script#__NEXT_DATA__").ok()?;
    let script = doc.select(&script_sel).next()?;
    let raw = script.text().collect::<String>();
    let value: Value = serde_json::from_str(&raw).ok()?;

    let jobs_val = find_first_key(&value, "jobs")?.as_array()?.to_owned();
    if jobs_val.is_empty() {
        return None;
    }

    let mut jobs = Vec::new();
    for item in &jobs_val {
        let id = item
            .get("id").or_else(|| item.get("jobId")).or_else(|| item.get("sourceId"))
            .and_then(|v| v.as_str().map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string())))
            .unwrap_or_default();
        if id.is_empty() { continue; }

        let title = item
            .get("title").or_else(|| item.get("jobTitle"))
            .and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if title.is_empty() { continue; }

        let raw_type = item
            .get("jobType").or_else(|| item.get("job_type")).or_else(|| item.get("type"))
            .and_then(|v| v.as_str()).unwrap_or("").to_string();
        let job_type = map_bruntwork_job_type(&raw_type);

        let posted_at = item
            .get("publishedOn").or_else(|| item.get("published_on"))
            .or_else(|| item.get("createdAt")).or_else(|| item.get("created_at"))
            .or_else(|| item.get("datePosted")).or_else(|| item.get("date_posted"))
            .or_else(|| item.get("postedDate")).or_else(|| item.get("updatedAt"))
            .and_then(|v| v.as_str()).unwrap_or("").to_string();

        let pay = item
            .get("salary").or_else(|| item.get("salaryMin")).or_else(|| item.get("salary_min"))
            .or_else(|| item.get("hourlyRate")).or_else(|| item.get("hourly_rate"))
            .or_else(|| item.get("compensation")).or_else(|| item.get("payRate"))
            .and_then(|v| v.as_str().map(|s| s.to_string())
                .or_else(|| v.as_f64().map(|n| format!("${n}"))))
            .unwrap_or_default();

        let url = format!("{BRUNTWORK_SITE_BASE}/jobs/{id}");
        jobs.push(Job {
            id: None,
            source: "bruntwork".to_string(),
            source_id: id,
            title,
            company: "BruntWork".to_string(),
            company_logo_url: String::new(),
            pay,
            posted_at,
            url,
            summary: String::new(),
            keyword: String::new(),
            scraped_at: now.to_string(),
            is_new: true,
            watchlisted: false,
            run_id: None,
            salary_min: None,
            salary_max: None,
            salary_currency: String::new(),
            salary_period: String::new(),
            applied: false,
            job_type,
        });
    }
    if jobs.is_empty() { None } else { Some(jobs) }
}

fn parse_bruntwork_search_html(html: &str, now: &str) -> Result<Vec<Job>, String> {
    let doc = Html::parse_document(html);
    let link_sel = Selector::parse("a[href*='/jobs/']").map_err(|e| e.to_string())?;

    let mut seen_ids = std::collections::HashSet::new();
    let mut jobs = Vec::new();

    for link in doc.select(&link_sel) {
        let href = link.value().attr("href").unwrap_or_default();
        // Skip apply links
        if href.contains("/apply") { continue; }

        let id = href
            .split("/jobs/").nth(1)
            .and_then(|s| s.split('/').next())
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || seen_ids.contains(&id) { continue; }

        let text = normalize_text(&link.text().collect::<String>());
        let (title, job_type) = split_bruntwork_title_type(&text);
        if title.is_empty() { continue; }

        // Bruntwork is a Next.js app; dates are in __NEXT_DATA__ JSON (handled by
        // try_parse_bruntwork_next_data). The HTML fallback has no reliable date element.
        let posted_at = String::new();

        seen_ids.insert(id.clone());
        let url = format!("{BRUNTWORK_SITE_BASE}/jobs/{id}");
        jobs.push(Job {
            id: None,
            source: "bruntwork".to_string(),
            source_id: id,
            title,
            company: "BruntWork".to_string(),
            company_logo_url: String::new(),
            pay: String::new(),
            posted_at,
            url,
            summary: String::new(),
            keyword: String::new(),
            scraped_at: now.to_string(),
            is_new: true,
            watchlisted: false,
            run_id: None,
            salary_min: None,
            salary_max: None,
            salary_currency: String::new(),
            salary_period: String::new(),
            applied: false,
            job_type,
        });
    }
    Ok(jobs)
}

fn split_bruntwork_title_type(combined: &str) -> (String, String) {
    for marker in &["Full Time", "Part Time", "Project Based"] {
        if let Some(pos) = combined.find(marker) {
            let title = combined[..pos].trim().to_string();
            let raw_type = combined[pos..].trim().to_string();
            if !title.is_empty() {
                return (title, map_bruntwork_job_type(&raw_type));
            }
        }
    }
    (combined.trim().to_string(), String::new())
}

fn map_bruntwork_job_type(raw: &str) -> String {
    let r = raw.trim();
    if r.starts_with("Full Time") || r.starts_with("Full-Time") {
        "Full-Time".to_string()
    } else if r.starts_with("Part Time") || r.starts_with("Part-Time") {
        if let Some(hrs) = extract_bruntwork_hour_range(r) {
            format!("Part-Time ({hrs} hrs/wk)")
        } else {
            "Part-Time".to_string()
        }
    } else if r.starts_with("Project Based") || r.starts_with("Project-Based") {
        "Contract".to_string()
    } else if !r.is_empty() {
        r.to_string()
    } else {
        String::new()
    }
}

/// "Part Time (20 - 34 Hours per week)" → Some("20-34")
/// "Part Time (10-19 Hours)"             → Some("10-19")
fn extract_bruntwork_hour_range(s: &str) -> Option<String> {
    let start = s.find('(')?;
    let end = s.find(')')?;
    let inner = s[start + 1..end].trim();
    let parts: Vec<&str> = inner.split_whitespace().collect();
    match parts.as_slice() {
        [a, "-", b, ..] => Some(format!("{a}-{b}")),
        [a, ..] if a.contains('-') => Some(a.to_string()),
        [a, ..] => Some(a.to_string()),
        _ => None,
    }
}

/// Decode a JSON-escaped string literal starting immediately after the opening `"`.
/// Returns the decoded string and how many bytes were consumed (including the closing `"`).
fn decode_json_string(s: &str) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let mut result = String::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some((result, i + 1)),
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                match bytes[i] {
                    b'"'  => result.push('"'),
                    b'\\' => result.push('\\'),
                    b'/'  => result.push('/'),
                    b'n'  => result.push('\n'),
                    b'r'  => result.push('\r'),
                    b't'  => result.push('\t'),
                    b'b'  => result.push('\x08'),
                    b'f'  => result.push('\x0c'),
                    b'u' if i + 4 < bytes.len() => {
                        if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 5]) {
                            if let Ok(code) = u32::from_str_radix(hex, 16) {
                                if let Some(ch) = char::from_u32(code) {
                                    result.push(ch);
                                }
                            }
                            i += 4;
                        }
                    }
                    c => result.push(c as char),
                }
            }
            c => result.push(c as char),
        }
        i += 1;
    }
    None
}

/// Extract description from a raw RSC payload (response body of a `RSC: 1` request).
/// Each line is of the form `ID:DATA` where DATA is JSON, a string, or an import ref.
fn try_parse_rsc_payload_description(payload: &str) -> Option<(String, String)> {
    for line in payload.lines() {
        let Some(colon) = line.find(':') else { continue };
        let id = &line[..colon];
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) { continue; }
        let data = &line[colon + 1..];
        if !data.starts_with('{') && !data.starts_with('[') { continue; }
        let Ok(val) = serde_json::from_str::<Value>(data) else { continue };

        for key in &["description", "descriptionHtml", "body", "content"] {
            if let Some(s) = find_first_key_string(&val, key) {
                if s.len() > 80 && !s.contains("self.__next_f") {
                    let is_html = s.contains('<') && s.contains('>');
                    return Some(if is_html {
                        (String::new(), s)
                    } else {
                        (normalize_text(&s), String::new())
                    });
                }
            }
        }
    }
    None
}

/// Try to extract the job description from Next.js App Router RSC streaming data
/// (`self.__next_f.push([1,"..."])` chunks embedded in the page HTML).
fn try_parse_bruntwork_rsc_description(html: &str) -> Option<(String, String)> {
    let prefix = "self.__next_f.push([1,\"";
    let mut search_pos = 0;

    while search_pos < html.len() {
        let rel = html[search_pos..].find(prefix)?;
        let content_start = search_pos + rel + prefix.len();
        let (chunk, consumed) = decode_json_string(&html[content_start..])?;
        search_pos = content_start + consumed;

        for line in chunk.lines() {
            // RSC line format: "ID:DATA" — skip import refs like "2:I[...]"
            let Some(colon) = line.find(':') else { continue };
            let data = &line[colon + 1..];
            if !data.starts_with('{') && !data.starts_with('[') {
                continue;
            }
            let Ok(val) = serde_json::from_str::<Value>(data) else { continue };

            for key in &["description", "descriptionHtml", "body", "content"] {
                if let Some(s) = find_first_key_string(&val, key) {
                    if s.len() > 80 && !s.contains("self.__next_f") {
                        let is_html = s.contains('<') && s.contains('>');
                        return Some(if is_html {
                            (String::new(), s)
                        } else {
                            (normalize_text(&s), String::new())
                        });
                    }
                }
            }
        }
    }
    None
}

/// Returns true if the text is Next.js RSC streaming garbage (not real content).
pub(crate) fn is_rsc_garbage(text: &str) -> bool {
    text.contains("self.__next_f") || text.contains("static/chunks/")
}

fn extract_bruntwork_published_date(html: &str) -> String {
    // Bruntwork job pages show "Published on" followed by a date like "Apr 10 2026".
    // On fully-rendered DOM output (WebView), scraper's `.text()` concatenates text
    // nodes with no separators, so splitting on newlines isn't reliable. Parse a
    // `<Month> <day> <year>` token explicitly instead.
    let text = Html::parse_document(html)
        .root_element()
        .text()
        .collect::<String>();
    let needle = "Published on";
    let Some(idx) = text.find(needle) else { return String::new() };
    parse_date_token(&text[idx + needle.len()..]).unwrap_or_default()
}

/// Parse a leading `<Month> <day> <year>` date token from `s`, e.g. `"Apr 13 2026"`
/// or `"April 13, 2026"`. Leading whitespace/punctuation is skipped. Returns the
/// normalized `"Mon D YYYY"` form, or `None` if the pattern doesn't match.
fn parse_date_token(s: &str) -> Option<String> {
    let s = s.trim_start_matches(|c: char| c.is_whitespace() || c == ':');
    let bytes = s.as_bytes();
    let mut i = 0;

    let month_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() && i - month_start < 9 {
        i += 1;
    }
    let month = &s[month_start..i];
    if !is_month_name(month) {
        return None;
    }

    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b',') {
        i += 1;
    }

    let day_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() && i - day_start < 2 {
        i += 1;
    }
    if i == day_start {
        return None;
    }
    let day: u32 = s[day_start..i].parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }

    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b',') {
        i += 1;
    }

    if i + 4 > bytes.len() {
        return None;
    }
    let year = &s[i..i + 4];
    if !year.as_bytes().iter().all(|b| b.is_ascii_digit()) || !year.starts_with('2') {
        return None;
    }

    let mon_abbr: String = month.chars().take(3).collect();
    let mon_abbr = {
        let mut c = mon_abbr.chars();
        match c.next() {
            Some(first) => first.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase(),
            None => String::new(),
        }
    };
    Some(format!("{mon_abbr} {day} {year}"))
}

fn is_month_name(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "jan" | "january"
            | "feb" | "february"
            | "mar" | "march"
            | "apr" | "april"
            | "may"
            | "jun" | "june"
            | "jul" | "july"
            | "aug" | "august"
            | "sep" | "sept" | "september"
            | "oct" | "october"
            | "nov" | "november"
            | "dec" | "december"
    )
}

pub(crate) fn parse_bruntwork_job_details(html: &str) -> Result<JobDetailsPayload, String> {
    let posted_at = extract_bruntwork_published_date(html);

    // 1. Try __NEXT_DATA__ (Next.js Pages Router — older sites)
    if let Some(mut payload) = try_parse_bruntwork_details_next_data(html) {
        if payload.posted_at.is_empty() { payload.posted_at = posted_at.clone(); }
        if is_meaningful_job_details(&payload) {
            return Ok(payload);
        }
    }

    // 2. Try RSC stream parser (Next.js App Router — current Bruntwork)
    if let Some((desc, desc_html)) = try_parse_bruntwork_rsc_description(html) {
        let job_type = infer_job_type("", if desc.is_empty() { &desc_html } else { &desc });
        return Ok(JobDetailsPayload {
            company: "BruntWork".to_string(),
            poster_name: String::new(),
            company_logo_url: String::new(),
            description: desc,
            description_html: desc_html,
            job_type,
            posted_at,
        });
    }

    // 3. Generic HTML parser — strip RSC script garbage from description
    let mut payload = parse_job_details(html)?;
    if payload.company.is_empty() { payload.company = "BruntWork".to_string(); }
    if payload.posted_at.is_empty() { payload.posted_at = posted_at; }
    // If description is RSC garbage, clear it so the drawer shows nothing instead
    if is_rsc_garbage(&payload.description) { payload.description = String::new(); }
    if is_rsc_garbage(&payload.description_html) { payload.description_html = String::new(); }
    Ok(payload)
}

fn try_parse_bruntwork_details_next_data(html: &str) -> Option<JobDetailsPayload> {
    let doc = Html::parse_document(html);
    let script_sel = Selector::parse("script#__NEXT_DATA__").ok()?;
    let script = doc.select(&script_sel).next()?;
    let raw = script.text().collect::<String>();
    let value: Value = serde_json::from_str(&raw).ok()?;

    let raw_desc = find_first_key_string(&value, "description")
        .or_else(|| find_first_key_string(&value, "descriptionHtml"))
        .or_else(|| find_first_key_string(&value, "description_html"))
        .or_else(|| find_first_key_string(&value, "body"))
        .or_else(|| find_first_key_string(&value, "content"))
        .unwrap_or_default();
    if raw_desc.is_empty() {
        return None;
    }

    // If the description contains HTML markup, route it to description_html so the
    // frontend renders it properly. Otherwise treat as plain text.
    let looks_like_html = raw_desc.contains('<') && raw_desc.contains('>');
    let (description, description_html) = if looks_like_html {
        (String::new(), raw_desc.trim().to_string())
    } else {
        (normalize_text(&raw_desc), String::new())
    };

    let text_for_inference = if description.is_empty() { &description_html } else { &description };
    let raw_type = find_first_key_string(&value, "jobType")
        .or_else(|| find_first_key_string(&value, "job_type"))
        .unwrap_or_default();
    let job_type = if raw_type.is_empty() {
        infer_job_type("", text_for_inference)
    } else {
        map_bruntwork_job_type(&raw_type)
    };

    let posted_at = find_first_key_string(&value, "publishedOn")
        .or_else(|| find_first_key_string(&value, "published_on"))
        .or_else(|| find_first_key_string(&value, "publishedAt"))
        .or_else(|| find_first_key_string(&value, "createdAt"))
        .or_else(|| find_first_key_string(&value, "created_at"))
        .or_else(|| find_first_key_string(&value, "datePosted"))
        .or_else(|| find_first_key_string(&value, "date_posted"))
        .unwrap_or_default();

    Some(JobDetailsPayload {
        company: "BruntWork".to_string(),
        poster_name: String::new(),
        company_logo_url: String::new(),
        description,
        description_html,
        job_type,
        posted_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn allows_only_https_onlinejobs_hosts() {
        assert!(is_allowed_job_url("https://www.onlinejobs.ph/jobseekers/job/123"));
        assert!(is_allowed_job_url("https://onlinejobs.ph/jobseekers/job/123"));
        assert!(is_allowed_job_url("https://www.bruntworkcareers.co/jobs/51936545689"));
        assert!(is_allowed_job_url("https://bruntworkcareers.co/jobs/51936545689"));
        assert!(!is_allowed_job_url("http://www.onlinejobs.ph/jobseekers/job/123"));
        assert!(!is_allowed_job_url("https://evil.example.com/jobseekers/job/123"));
        assert!(!is_allowed_job_url("javascript:alert(1)"));
    }

    #[test]
    fn parse_allowed_job_url_rejects_querystring_host_spoofing() {
        assert!(parse_allowed_job_url(
            "https://evil.example.com/path/bruntworkcareers.co/jobs/1?next=https://www.bruntworkcareers.co/jobs/2"
        )
        .is_err());
    }

    #[test]
    fn bruntwork_detection_uses_parsed_host() {
        let parsed = parse_allowed_job_url("https://www.bruntworkcareers.co/jobs/51936545689")
            .expect("expected allowlisted bruntwork url");
        assert!(is_bruntwork_job_url(&parsed));

        let parsed = parse_allowed_job_url("https://www.onlinejobs.ph/jobseekers/job/123")
            .expect("expected allowlisted onlinejobs url");
        assert!(!is_bruntwork_job_url(&parsed));
    }

    #[test]
    fn posted_at_days_ago_relative() {
        let now = Utc.with_ymd_and_hms(2026, 4, 16, 12, 0, 0).unwrap();
        assert_eq!(posted_at_days_ago("today", &now), Some(0));
        assert_eq!(posted_at_days_ago("just now", &now), Some(0));
        assert_eq!(posted_at_days_ago("3 hours ago", &now), Some(0));
        assert_eq!(posted_at_days_ago("yesterday", &now), Some(1));
        assert_eq!(posted_at_days_ago("3 days ago", &now), Some(3));
        assert_eq!(posted_at_days_ago("1 week ago", &now), Some(7));
        assert_eq!(posted_at_days_ago("2 weeks ago", &now), Some(14));
        assert_eq!(posted_at_days_ago("1 month ago", &now), Some(30));
        assert_eq!(posted_at_days_ago("2 months ago", &now), Some(60));
    }

    #[test]
    fn posted_at_days_ago_absolute() {
        let now = Utc.with_ymd_and_hms(2026, 4, 16, 12, 0, 0).unwrap();
        assert_eq!(posted_at_days_ago("April 16, 2026", &now), Some(0));
        assert_eq!(posted_at_days_ago("April 15, 2026", &now), Some(1));
        assert_eq!(posted_at_days_ago("April 13, 2026", &now), Some(3));
        assert_eq!(posted_at_days_ago("April 9, 2026", &now), Some(7));
        assert_eq!(posted_at_days_ago("January 1, 2026", &now), Some(105));
        assert_eq!(posted_at_days_ago("Apr 15, 2026", &now), Some(1));
    }

    #[test]
    fn posted_at_days_ago_iso() {
        let now = Utc.with_ymd_and_hms(2026, 4, 16, 12, 0, 0).unwrap();
        assert_eq!(posted_at_days_ago("2026-04-16 05:20:50", &now), Some(0));
        assert_eq!(posted_at_days_ago("2026-04-15 22:09:14", &now), Some(1));
        assert_eq!(posted_at_days_ago("2026-04-13 10:00:00", &now), Some(3));
        assert_eq!(posted_at_days_ago("2026-04-09 08:00:00", &now), Some(7));
        assert_eq!(posted_at_days_ago("2026-04-16", &now), Some(0));
        assert_eq!(posted_at_days_ago("2026-04-14", &now), Some(2));
    }

    #[test]
    fn posted_at_days_ago_unknown_returns_none() {
        let now = Utc.with_ymd_and_hms(2026, 4, 16, 12, 0, 0).unwrap();
        assert_eq!(posted_at_days_ago("", &now), None);
        assert_eq!(posted_at_days_ago("some random text", &now), None);
    }

    #[test]
    fn normalize_text_collapses_whitespace() {
        assert_eq!(normalize_text("  foo   bar\tbaz\n\nqux  "), "foo bar baz qux");
        assert_eq!(normalize_text(""), "");
        assert_eq!(normalize_text("   "), "");
        assert_eq!(normalize_text("single"), "single");
    }

    #[test]
    fn map_employment_type_canonicalises() {
        assert_eq!(map_employment_type("FULL_TIME"), "Full-Time");
        assert_eq!(map_employment_type("full-time"), "Full-Time");
        assert_eq!(map_employment_type("fulltime"), "Full-Time");
        assert_eq!(map_employment_type("PART_TIME"), "Part-Time");
        assert_eq!(map_employment_type("part-time"), "Part-Time");
        assert_eq!(map_employment_type("Contractor"), "Contract");
        assert_eq!(map_employment_type("contract"), "Contract");
        assert_eq!(map_employment_type("freelance"), "Contract");
        assert_eq!(map_employment_type("INTERN"), "");
        assert_eq!(map_employment_type(""), "");
    }

    #[test]
    fn extract_weekly_hours_matches_common_phrases() {
        assert_eq!(extract_weekly_hours("20 hours/week"), Some(20));
        assert_eq!(extract_weekly_hours("40 hours per week"), Some(40));
        assert_eq!(extract_weekly_hours("35 hours a week"), Some(35));
        assert_eq!(extract_weekly_hours("15 hrs/wk"), Some(15));
        assert_eq!(extract_weekly_hours("22 hours weekly"), Some(22));
        assert_eq!(extract_weekly_hours("no hours here"), None);
        assert_eq!(extract_weekly_hours("worked 5 hours on the task"), None);
        assert_eq!(extract_weekly_hours("200 hours/week"), None);
    }

    #[test]
    fn infer_job_type_prefers_explicit_labels() {
        assert_eq!(infer_job_type("Full-Time SEO Specialist", ""), "Full-Time");
        assert_eq!(infer_job_type("Part Time VA", ""), "Part-Time");
        assert_eq!(
            infer_job_type("SEO Specialist", "This is a full-time role, 40 hours per week."),
            "Full-Time (40 hrs/wk)",
        );
        assert_eq!(
            infer_job_type("Admin", "part-time, 20 hours/week"),
            "Part-Time (20 hrs/wk)",
        );
    }

    #[test]
    fn infer_job_type_uses_hours_when_no_label() {
        assert_eq!(
            infer_job_type("Manager", "40 hours per week"),
            "Full-Time (40 hrs/wk)",
        );
        assert_eq!(
            infer_job_type("Manager", "20 hours per week"),
            "Part-Time (20 hrs/wk)",
        );
        assert_eq!(infer_job_type("Manager", ""), "");
    }

    #[test]
    fn is_meaningful_job_details_requires_some_content() {
        let empty = JobDetailsPayload {
            company: String::new(),
            poster_name: String::new(),
            company_logo_url: String::new(),
            description: String::new(),
            description_html: String::new(),
            job_type: String::new(),
            posted_at: String::new(),
        };
        assert!(!is_meaningful_job_details(&empty));

        let with_desc = JobDetailsPayload {
            description: "some text".to_string(),
            ..empty.clone()
        };
        assert!(is_meaningful_job_details(&with_desc));

        let with_html = JobDetailsPayload {
            description_html: "<p>hi</p>".to_string(),
            ..empty.clone()
        };
        assert!(is_meaningful_job_details(&with_html));

        let with_company = JobDetailsPayload {
            company: "Acme".to_string(),
            ..empty.clone()
        };
        assert!(is_meaningful_job_details(&with_company));

        // Whitespace-only fields do not qualify.
        let whitespace = JobDetailsPayload {
            description: "   ".to_string(),
            description_html: "\n\t".to_string(),
            company: "  ".to_string(),
            ..empty
        };
        assert!(!is_meaningful_job_details(&whitespace));
    }

    #[test]
    fn parse_date_token_accepts_common_formats() {
        assert_eq!(parse_date_token("Apr 13 2026").as_deref(), Some("Apr 13 2026"));
        assert_eq!(parse_date_token("April 13, 2026").as_deref(), Some("Apr 13 2026"));
        assert_eq!(parse_date_token(": April 5 2026").as_deref(), Some("Apr 5 2026"));
        assert_eq!(parse_date_token("  May 1 2026").as_deref(), Some("May 1 2026"));
        assert_eq!(parse_date_token("January 31, 2026").as_deref(), Some("Jan 31 2026"));
    }

    #[test]
    fn parse_date_token_rejects_bad_inputs() {
        assert_eq!(parse_date_token(""), None);
        assert_eq!(parse_date_token("Foo 13 2026"), None);
        assert_eq!(parse_date_token("Apr 13 1999"), None); // year must start with '2'
        assert_eq!(parse_date_token("Apr XX 2026"), None);
        assert_eq!(parse_date_token("Apr 13"), None);
    }

    #[test]
    fn split_bruntwork_title_type_splits_on_marker() {
        assert_eq!(
            split_bruntwork_title_type("SEO Specialist Full Time (40 hours per week)"),
            ("SEO Specialist".to_string(), "Full-Time".to_string()),
        );
        assert_eq!(
            split_bruntwork_title_type("Admin VA Part Time (20 - 30 Hours per week)"),
            ("Admin VA".to_string(), "Part-Time (20-30 hrs/wk)".to_string()),
        );
        assert_eq!(
            split_bruntwork_title_type("Data Engineer Project Based"),
            ("Data Engineer".to_string(), "Contract".to_string()),
        );
        assert_eq!(
            split_bruntwork_title_type("Just a title"),
            ("Just a title".to_string(), String::new()),
        );
    }

    #[test]
    fn map_bruntwork_job_type_maps_variants() {
        assert_eq!(map_bruntwork_job_type("Full Time (40 hrs)"), "Full-Time");
        assert_eq!(map_bruntwork_job_type("Full-Time"), "Full-Time");
        assert_eq!(map_bruntwork_job_type("Part Time"), "Part-Time");
        assert_eq!(
            map_bruntwork_job_type("Part Time (20 - 30 Hours per week)"),
            "Part-Time (20-30 hrs/wk)",
        );
        assert_eq!(
            map_bruntwork_job_type("Part Time (10-19 Hours)"),
            "Part-Time (10-19 hrs/wk)",
        );
        assert_eq!(map_bruntwork_job_type("Project Based"), "Contract");
        assert_eq!(map_bruntwork_job_type("Project-Based"), "Contract");
        assert_eq!(map_bruntwork_job_type(""), "");
        assert_eq!(map_bruntwork_job_type("Something Else"), "Something Else");
    }

    #[test]
    fn extract_bruntwork_hour_range_handles_formats() {
        assert_eq!(
            extract_bruntwork_hour_range("Part Time (20 - 34 Hours per week)"),
            Some("20-34".to_string()),
        );
        assert_eq!(
            extract_bruntwork_hour_range("Part Time (10-19 Hours)"),
            Some("10-19".to_string()),
        );
        assert_eq!(
            extract_bruntwork_hour_range("Part Time (40 Hours)"),
            Some("40".to_string()),
        );
        assert_eq!(extract_bruntwork_hour_range("Part Time 20 hours"), None);
        assert_eq!(extract_bruntwork_hour_range("Part Time ()"), None);
    }

    #[test]
    fn is_rsc_garbage_detects_next_streaming_chunks() {
        assert!(is_rsc_garbage("self.__next_f.push([1,\"...\"])"));
        assert!(is_rsc_garbage("look at static/chunks/main.js"));
        assert!(!is_rsc_garbage("This is a normal job description."));
        assert!(!is_rsc_garbage(""));
    }

    #[test]
    fn decode_json_string_handles_escapes() {
        let (s, _) = decode_json_string(r#"hello\nworld""#).unwrap();
        assert_eq!(s, "hello\nworld");

        let (s, _) = decode_json_string(r#"a\tb\\c""#).unwrap();
        assert_eq!(s, "a\tb\\c");

        let (s, _) = decode_json_string(r#"\u0041BC""#).unwrap();
        assert_eq!(s, "ABC");

        // Escaped closing quote is content, not terminator — so an unterminated
        // input returns None.
        assert!(decode_json_string(r#"hello\nworld\""#).is_none());
        assert!(decode_json_string(r#"no closing quote"#).is_none());
    }

    #[test]
    fn parse_search_page_extracts_onlinejobs_card() {
        let html = r#"
            <html><body>
              <div class="jobpost-cat-box">
                <div class="jobpost-cat-box-logo">
                  <img src="https://example.com/logo.png" alt="Acme Corp">
                </div>
                <h4>SEO Specialist</h4>
                <p class="fs-13"><em>Posted on Apr 15, 2026</em></p>
                <dl class="no-gutters"><dd>$8/hr</dd></dl>
                <div class="desc">Looking for a full-time SEO specialist, 40 hours per week.</div>
                <a href="/jobseekers/job/seo-specialist-123456">View</a>
              </div>
              <div class="jobpost-cat-box">
                <h4></h4>
                <a href="/jobseekers/job/empty-999">Empty</a>
              </div>
            </body></html>
        "#;
        let jobs = parse_search_page(html, "seo").unwrap();
        assert_eq!(jobs.len(), 1, "empty-title cards are skipped");
        let job = &jobs[0];
        assert_eq!(job.source, "onlinejobs");
        assert_eq!(job.title, "SEO Specialist");
        assert_eq!(job.company, "Acme Corp");
        assert_eq!(job.company_logo_url, "https://example.com/logo.png");
        assert_eq!(job.pay, "$8/hr");
        assert_eq!(job.posted_at, "Apr 15, 2026");
        assert_eq!(job.source_id, "123456");
        assert_eq!(job.url, "https://www.onlinejobs.ph/jobseekers/job/seo-specialist-123456");
        assert_eq!(job.keyword, "seo");
        assert!(job.summary.contains("full-time SEO specialist"));
        assert_eq!(job.job_type, "Full-Time (40 hrs/wk)");
        assert!(job.is_new);
    }

    #[test]
    fn parse_search_page_rejects_disallowed_urls() {
        let html = r#"
            <div class="jobpost-cat-box">
              <h4>Suspicious Job</h4>
              <a href="https://evil.example.com/jobseekers/job/1">x</a>
            </div>
        "#;
        let jobs = parse_search_page(html, "k").unwrap();
        assert!(jobs.is_empty(), "non-allowlisted hosts must be dropped");
    }

    #[test]
    fn parse_job_details_reads_jsonld() {
        let html = r#"
            <html><head>
              <script type="application/ld+json">
              {
                "@type": "JobPosting",
                "title": "SEO Specialist",
                "description": "Work on technical SEO.  Full-time role.",
                "employmentType": "FULL_TIME",
                "hiringOrganization": {
                  "@type": "Organization",
                  "name": "Acme Corp",
                  "logo": "https://example.com/logo.png"
                },
                "author": {"@type": "Person", "name": "Jane Doe"}
              }
              </script>
            </head><body>
              <div class="job-description">Ignored because JSON-LD already set description.</div>
            </body></html>
        "#;
        let payload = parse_job_details(html).unwrap();
        assert_eq!(payload.company, "Acme Corp");
        assert_eq!(payload.company_logo_url, "https://example.com/logo.png");
        assert_eq!(payload.poster_name, "Jane Doe");
        assert_eq!(payload.description, "Work on technical SEO. Full-time role.");
        assert_eq!(payload.job_type, "Full-Time");
    }

    #[test]
    fn parse_job_details_falls_back_to_css() {
        let html = r#"
            <html><body>
              <div class="company-name">Beta LLC</div>
              <div class="job-description">
                This is a long enough description to pass the 60-char threshold
                required by extract_longest_text, talking about a part-time role
                with 20 hours per week of focused SEO work.
              </div>
            </body></html>
        "#;
        let payload = parse_job_details(html).unwrap();
        assert_eq!(payload.company, "Beta LLC");
        assert!(payload.description.contains("part-time role"));
        assert_eq!(payload.job_type, "Part-Time (20 hrs/wk)");
    }

    #[test]
    fn parse_bruntwork_search_html_extracts_links() {
        let html = r#"
            <html><body>
              <ul>
                <li><a href="/jobs/123">SEO Specialist Full Time (40 hrs)</a></li>
                <li><a href="/jobs/456">Admin VA Part Time (20 - 30 Hours per week)</a></li>
                <li><a href="/jobs/789/apply">Apply link should be skipped</a></li>
                <li><a href="/jobs/abc">Non-numeric id skipped</a></li>
                <li><a href="/jobs/123">Duplicate id skipped</a></li>
                <li><a href="/jobs/999">Data Engineer Project Based</a></li>
              </ul>
            </body></html>
        "#;
        let jobs = parse_bruntwork_search_html(html, "2026-04-21T00:00:00Z").unwrap();
        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs[0].source, "bruntwork");
        assert_eq!(jobs[0].company, "BruntWork");
        assert_eq!(jobs[0].source_id, "123");
        assert_eq!(jobs[0].title, "SEO Specialist");
        assert_eq!(jobs[0].job_type, "Full-Time");
        assert_eq!(jobs[0].url, format!("{BRUNTWORK_SITE_BASE}/jobs/123"));

        assert_eq!(jobs[1].source_id, "456");
        assert_eq!(jobs[1].title, "Admin VA");
        assert_eq!(jobs[1].job_type, "Part-Time (20-30 hrs/wk)");

        assert_eq!(jobs[2].source_id, "999");
        assert_eq!(jobs[2].title, "Data Engineer");
        assert_eq!(jobs[2].job_type, "Contract");
    }

    #[test]
    fn parse_bruntwork_job_details_uses_next_data_description() {
        let html = r#"
            <html><head>
              <script id="__NEXT_DATA__" type="application/json">
              {"props":{"pageProps":{"job":{
                "description":"We are hiring a full-time SEO specialist working 40 hours per week.",
                "jobType":"Full Time",
                "publishedOn":"2026-04-10T00:00:00Z"
              }}}}
              </script>
            </head><body>
              <main>Published on Apr 10 2026</main>
            </body></html>
        "#;
        let payload = parse_bruntwork_job_details(html).unwrap();
        assert_eq!(payload.company, "BruntWork");
        assert!(payload.description.contains("full-time SEO specialist"));
        assert_eq!(payload.job_type, "Full-Time");
        // posted_at is filled from __NEXT_DATA__ when present.
        assert_eq!(payload.posted_at, "2026-04-10T00:00:00Z");
    }

    #[test]
    fn parse_bruntwork_job_details_extracts_published_date_fallback() {
        // No __NEXT_DATA__, no RSC — falls through to generic parser. The published
        // date should still be extracted from the rendered "Published on …" text.
        let html = r#"
            <html><body>
              <div>Published on Apr 10 2026</div>
              <div class="job-description">
                Long enough description to pass threshold: we are looking for a
                skilled SEO professional with at least three years of experience.
              </div>
            </body></html>
        "#;
        let payload = parse_bruntwork_job_details(html).unwrap();
        assert_eq!(payload.company, "BruntWork");
        assert_eq!(payload.posted_at, "Apr 10 2026");
        assert!(payload.description.contains("SEO professional"));
    }
}
