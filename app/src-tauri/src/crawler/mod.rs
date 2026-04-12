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
const CRAWL_DELAY: Duration = Duration::from_secs(5);
const MAX_PAGES: usize = 5;
const FETCH_MAX_ATTEMPTS: usize = 3;
const FETCH_RETRY_BASE_DELAY: Duration = Duration::from_millis(700);
const SCRAPLING_ENABLE_ENV: &str = "EZER_ENABLE_SCRAPLING_FALLBACK";
const SCRAPLING_BASE_URL_ENV: &str = "EZER_SCRAPLING_BASE_URL";

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
        let enabled = std::env::var(SCRAPLING_ENABLE_ENV).unwrap_or_default() == "1";
        if !enabled {
            return None;
        }
        let base = std::env::var(SCRAPLING_BASE_URL_ENV).ok()?;
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
                summary: row
                    .summary
                    .map(|s| normalize_text(&s))
                    .unwrap_or_default(),
                keyword: keyword.to_string(),
                scraped_at: now.clone(),
                is_new: true,
                watchlisted: false,
                run_id: None,
                salary_min: None,
                salary_max: None,
                salary_currency: String::new(),
                salary_period: String::new(),
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
        Some(JobDetailsPayload {
            company: data.company.map(|s| normalize_text(&s)).unwrap_or_default(),
            poster_name: data.poster_name.map(|s| normalize_text(&s)).unwrap_or_default(),
            company_logo_url: data
                .company_logo_url
                .map(|s| normalize_asset_url(&s))
                .unwrap_or_default(),
            description: data.description.map(|s| normalize_text(&s)).unwrap_or_default(),
            description_html: data.description_html.unwrap_or_default(),
        })
    }

    pub async fn fetch_job_details(&self, url: &str) -> Result<JobDetailsPayload, String> {
        if !is_allowed_job_url(url) {
            return Err("Unsupported job URL".to_string());
        }
        let html = self.fetch_with_retry(url).await?;
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

    Ok(JobDetailsPayload {
        company,
        poster_name,
        company_logo_url,
        description,
        description_html,
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

fn is_meaningful_job_details(payload: &JobDetailsPayload) -> bool {
    !payload.description.trim().is_empty()
        || !payload.description_html.trim().is_empty()
        || !payload.company.trim().is_empty()
}

fn is_allowed_job_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    matches!(
        parsed.host_str(),
        Some("onlinejobs.ph") | Some("www.onlinejobs.ph")
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

#[cfg(test)]
mod tests {
    use super::is_allowed_job_url;

    #[test]
    fn allows_only_https_onlinejobs_hosts() {
        assert!(is_allowed_job_url("https://www.onlinejobs.ph/jobseekers/job/123"));
        assert!(is_allowed_job_url("https://onlinejobs.ph/jobseekers/job/123"));
        assert!(!is_allowed_job_url("http://www.onlinejobs.ph/jobseekers/job/123"));
        assert!(!is_allowed_job_url("https://evil.example.com/jobseekers/job/123"));
        assert!(!is_allowed_job_url("javascript:alert(1)"));
    }
}
