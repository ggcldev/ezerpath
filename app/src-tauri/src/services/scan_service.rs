use crate::crawler::{Crawler, CrawlStats, ScanProgress};
use crate::db::Database;
use std::sync::Arc;
use tauri::ipc::Channel;
use tokio::sync::Mutex;

pub async fn run_crawl(
    db: &Arc<Database>,
    crawler: &Crawler,
    crawl_lock: &Mutex<()>,
    days: Option<u32>,
    sources: Option<&[String]>,
    on_progress: Option<&Channel<ScanProgress>>,
) -> Result<Vec<CrawlStats>, String> {
    let scan_onlinejobs = sources.map_or(true, |s| s.iter().any(|x| x == "onlinejobs"));
    let scan_bruntwork = sources.map_or(true, |s| s.iter().any(|x| x == "bruntwork"));
    let _crawl_guard = crawl_lock
        .try_lock()
        .map_err(|_| "A scan is already in progress".to_string())?;

    let date_days = days.unwrap_or(3);
    let keywords = db.get_keywords().map_err(|e| e.to_string())?;

    let started_at = chrono::Utc::now().to_rfc3339();
    let keywords_str = keywords.join(", ");
    let run_id = db
        .insert_run(&keywords_str, &started_at)
        .map_err(|e| e.to_string())?;

    if let Some(ch) = on_progress {
        let _ = ch.send(ScanProgress::Started {
            run_id,
            total_keywords: keywords.len(),
            keywords: keywords.clone(),
        });
    }

    let mut all_stats: Vec<CrawlStats> = Vec::new();
    let mut total_found: i64 = 0;
    let mut total_new: i64 = 0;

    if scan_onlinejobs {
        for (idx, kw) in keywords.iter().enumerate() {
            if let Some(ch) = on_progress {
                let _ = ch.send(ScanProgress::KeywordStarted {
                    keyword: kw.clone(),
                    index: idx,
                    total: keywords.len(),
                });
            }

            match crawler.crawl_keyword(kw, db, date_days, run_id, on_progress).await {
                Ok(stats) => {
                    total_found += stats.found as i64;
                    total_new += stats.new as i64;

                    if let Some(ch) = on_progress {
                        let _ = ch.send(ScanProgress::KeywordCompleted {
                            keyword: kw.clone(),
                            found: stats.found,
                            new: stats.new,
                            pages: stats.pages,
                        });
                    }

                    all_stats.push(stats);
                }
                Err(err) => {
                    let finished_at = chrono::Utc::now().to_rfc3339();
                    if let Err(mark_err) =
                        db.fail_run(run_id, total_found, total_new, &err, &finished_at)
                    {
                        let combined = format!("{err} (failed to mark run failed: {mark_err})");
                        if let Some(ch) = on_progress {
                            let _ = ch.send(ScanProgress::Failed {
                                run_id,
                                error: combined.clone(),
                            });
                        }
                        return Err(combined);
                    }
                    if let Some(ch) = on_progress {
                        let _ = ch.send(ScanProgress::Failed {
                            run_id,
                            error: err.clone(),
                        });
                    }
                    return Err(err);
                }
            }
        }
    }

    if scan_bruntwork {
        let bw_stats = crawler
            .crawl_bruntwork(&keywords, db, run_id, on_progress)
            .await;
        for s in &bw_stats {
            total_found += s.found as i64;
            total_new += s.new as i64;
        }
        all_stats.extend(bw_stats);
    }

    let finished_at = chrono::Utc::now().to_rfc3339();
    db.complete_run(run_id, total_found, total_new, &finished_at)
        .map_err(|e| e.to_string())?;

    if let Some(ch) = on_progress {
        let _ = ch.send(ScanProgress::Completed {
            run_id,
            total_found,
            total_new,
        });
    }

    Ok(all_stats)
}
