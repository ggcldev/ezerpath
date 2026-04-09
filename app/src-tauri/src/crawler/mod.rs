use crate::db::{Database, Job};
use chrono::Utc;
use reqwest::Client;
use scraper::{Html, Selector};
use std::sync::Arc;
use std::time::Duration;

const BASE_URL: &str = "https://www.onlinejobs.ph/jobseekers/jobsearch";
const CRAWL_DELAY: Duration = Duration::from_secs(5);
const MAX_PAGES: usize = 5;

pub struct Crawler {
    client: Client,
}

impl Crawler {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .user_agent("ezerpath/1.0 (+personal research crawler)")
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { client })
    }

    pub async fn crawl_keyword(&self, keyword: &str, db: &Arc<Database>, days: u32, run_id: i64) -> Result<CrawlStats, String> {
        let mut stats = CrawlStats { keyword: keyword.to_string(), found: 0, new: 0, pages: 0 };
        let encoded = urlencoding::encode(keyword);

        for page_num in 0..MAX_PAGES {
            let offset = page_num * 30;
            let url = if offset == 0 {
                format!("{}?jobkeyword={}&dateposted={}", BASE_URL, encoded, days)
            } else {
                format!("{}/{}?jobkeyword={}&dateposted={}", BASE_URL, offset, encoded, days)
            };

            let html = self.fetch(&url).await?;
            let jobs = parse_search_page(&html, keyword)?;

            if jobs.is_empty() {
                break;
            }

            stats.pages += 1;
            for job in &jobs {
                stats.found += 1;
                if db.insert_job(job, run_id).map_err(|e| e.to_string())? {
                    stats.new += 1;
                }
            }

            tokio::time::sleep(CRAWL_DELAY).await;
        }

        Ok(stats)
    }

    async fn fetch(&self, url: &str) -> Result<String, String> {
        let resp = self.client.get(url).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        resp.text().await.map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CrawlStats {
    pub keyword: String,
    pub found: usize,
    pub new: usize,
    pub pages: usize,
}

fn parse_search_page(html: &str, keyword: &str) -> Result<Vec<Job>, String> {
    let doc = Html::parse_document(html);
    let card_sel = Selector::parse(".jobpost-cat-box").map_err(|e| e.to_string())?;
    let title_sel = Selector::parse("h4").map_err(|e| e.to_string())?;
    let date_sel = Selector::parse("p.fs-13 em").map_err(|e| e.to_string())?;
    let pay_sel  = Selector::parse("dl.no-gutters dd").map_err(|e| e.to_string())?;
    let desc_sel = Selector::parse(".desc").map_err(|e| e.to_string())?;
    let logo_sel = Selector::parse(".jobpost-cat-box-logo").map_err(|e| e.to_string())?;
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
            .replace("Posted on ", "");

        let company = card.select(&logo_sel)
            .next()
            .and_then(|e| e.value().attr("alt"))
            .unwrap_or("")
            .to_string();

        let pay = card.select(&pay_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let summary = card.select(&desc_sel)
            .next()
            .map(|e| {
                let text: String = e.text().collect::<String>().trim().to_string();
                if text.len() > 500 { text[..500].to_string() } else { text }
            })
            .unwrap_or_default();

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
            pay,
            posted_at,
            url,
            summary,
            keyword: keyword.to_string(),
            scraped_at: now.clone(),
            is_new: true,
            watchlisted: false,
            run_id: None,
        });
    }

    Ok(jobs)
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

#[cfg(test)]
mod tests {
    use super::is_allowed_job_url;

    #[test]
    fn allows_only_https_onlinejobs_hosts() {
        assert!(is_allowed_job_url(
            "https://www.onlinejobs.ph/jobseekers/job/12345"
        ));
        assert!(is_allowed_job_url(
            "https://onlinejobs.ph/jobseekers/job/12345"
        ));
        assert!(!is_allowed_job_url(
            "http://www.onlinejobs.ph/jobseekers/job/12345"
        ));
        assert!(!is_allowed_job_url(
            "https://evil.example.com/jobseekers/job/12345"
        ));
        assert!(!is_allowed_job_url("javascript:alert(1)"));
    }
}
