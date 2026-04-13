// Golden-query evaluation harness (phase #5).
//
// Loads a frozen snapshot of ~20 jobs from eval/snapshot_jobs.json into a
// temp DB, runs each entry from eval/golden_queries.json through the
// matching retrieval primitive, and asserts that average recall@5 stays
// above the threshold. The harness exercises retrieval only — it does not
// call the LLM. Add new queries to eval/golden_queries.json when you find
// a regression in the field; reproducing the failure here is the entry
// point for fixing it.

use chrono::Utc;
use ezerpath_lib::db::{build_fts5_query, Database, Job};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const RECALL_THRESHOLD: f64 = 0.8;
const RECALL_K: usize = 5;

#[derive(Debug, Deserialize)]
struct SnapshotJob {
    source_id: String,
    title: String,
    company: String,
    pay: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct GoldenQuery {
    id: String,
    #[allow(dead_code)]
    query: String,
    route: String,
    n: usize,
    #[serde(default)]
    title_terms: Vec<String>,
    #[serde(default)]
    search_query: String,
    expected_source_ids: Vec<String>,
}

fn load_snapshot(db: &Database) -> HashMap<String, i64> {
    let raw = include_str!("../eval/snapshot_jobs.json");
    let snapshot: Vec<SnapshotJob> = serde_json::from_str(raw).expect("snapshot json");
    let now = Utc::now().to_rfc3339();
    let run_id = db
        .insert_run("eval", &now)
        .expect("insert run");
    for sj in &snapshot {
        let job = Job {
            id: None,
            source: "eval".to_string(),
            source_id: sj.source_id.clone(),
            title: sj.title.clone(),
            company: sj.company.clone(),
            company_logo_url: String::new(),
            pay: sj.pay.clone(),
            posted_at: now.clone(),
            url: format!("https://example.com/{}", sj.source_id),
            summary: sj.summary.clone(),
            keyword: String::new(),
            scraped_at: now.clone(),
            is_new: true,
            watchlisted: false,
            run_id: None,
            salary_min: None,
            salary_max: None,
            salary_currency: String::new(),
            salary_period: String::new(),
        };
        db.insert_job(&job, run_id).expect("insert eval job");
    }
    let mut map = HashMap::new();
    for j in db.get_jobs(None, false, None).expect("get_jobs") {
        if let Some(id) = j.id {
            map.insert(j.source_id.clone(), id);
        }
    }
    map
}

fn recall_at_k(
    retrieved: &[i64],
    expected_source_ids: &[String],
    src_to_id: &HashMap<String, i64>,
    k: usize,
) -> f64 {
    let expected: HashSet<i64> = expected_source_ids
        .iter()
        .filter_map(|s| src_to_id.get(s).copied())
        .collect();
    if expected.is_empty() {
        return 0.0;
    }
    let top_k: HashSet<i64> = retrieved.iter().take(k).copied().collect();
    let hits = expected.intersection(&top_k).count();
    hits as f64 / expected.len() as f64
}

#[test]
fn fts5_query_builder_handles_natural_language() {
    // Sanity-check the sanitizer used by the SearchKeyword route.
    assert_eq!(build_fts5_query("link building"), "link* building*");
    assert_eq!(build_fts5_query("react"), "react*");
    assert_eq!(build_fts5_query(""), "");
}

#[test]
fn golden_queries_meet_recall_threshold() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db = Database::new(tmp.path().to_path_buf()).expect("db");
    let src_to_id = load_snapshot(&db);

    let raw = include_str!("../eval/golden_queries.json");
    let queries: Vec<GoldenQuery> = serde_json::from_str(raw).expect("queries json");
    assert!(!queries.is_empty(), "no golden queries loaded");

    let mut total_recall = 0.0;
    let mut failures: Vec<String> = Vec::new();

    println!("\n=== Golden Query Eval ({} queries) ===", queries.len());
    for q in &queries {
        let retrieved: Vec<i64> = match q.route.as_str() {
            "ranking" => db
                .get_top_paying_jobs(None, &q.title_terms, q.n)
                .expect("get_top_paying_jobs")
                .into_iter()
                .filter_map(|j| j.id)
                .collect(),
            "search_keyword" => db
                .search_jobs_fts(&q.search_query, q.n)
                .expect("search_jobs_fts")
                .into_iter()
                .filter_map(|j| j.id)
                .collect(),
            other => panic!("unknown route '{}' in golden query '{}'", other, q.id),
        };
        let recall = recall_at_k(&retrieved, &q.expected_source_ids, &src_to_id, RECALL_K);
        let mark = if recall >= RECALL_THRESHOLD { "✓" } else { "✗" };
        println!(
            "  {} [{:>14}] {:<22}  recall@{} = {:.2}",
            mark, q.route, q.id, RECALL_K, recall
        );
        total_recall += recall;
        if recall < RECALL_THRESHOLD {
            failures.push(format!("{} ({:.2})", q.id, recall));
        }
    }

    let avg = total_recall / queries.len() as f64;
    println!(
        "\n  AVERAGE recall@{}: {:.2}  (threshold {:.2})",
        RECALL_K, avg, RECALL_THRESHOLD
    );
    println!("======================================\n");

    assert!(
        avg >= RECALL_THRESHOLD,
        "average recall@{} = {:.2} (threshold {:.2}). Per-query failures: {:?}",
        RECALL_K, avg, RECALL_THRESHOLD, failures
    );
}
