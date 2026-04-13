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
use ezerpath_lib::ai::followup::{resolve_followup, FollowUpAction};
use ezerpath_lib::ai::ranking::rank_embeddings_against_query;
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

// ─────────────────────────────────────────────────────────────────────────
// Semantic fallback eval
//
// The SearchKeyword route falls through to cosine similarity over cached
// job embeddings when FTS5 returns too few hits. That path is partly HTTP
// (sentence_service.embed_texts) and partly pure ranking
// (rank_embeddings_against_query + DB storage). The HTTP hop is awkward to
// exercise in-process, so this test pins down the deterministic half:
//
//   1. Seed a tiny DB with real rows.
//   2. Upsert hand-crafted vectors into job_embeddings.
//   3. For each query, pass a hand-crafted query vector through
//      list_job_embeddings → rank_embeddings_against_query.
//   4. Assert the top-ranked job matches the expected source_id.
//
// The vectors are 4-dim semantic-axis stand-ins. Each job gets weight on
// the axes it conceptually belongs to; queries are vectors along a single
// axis and should pull in the jobs with the highest weight on that axis.
// Axes: [0]=seo/marketing, [1]=dev/code, [2]=finance, [3]=design.
// ─────────────────────────────────────────────────────────────────────────

const SEMANTIC_MODEL: &str = "eval-synthetic";

fn seed_semantic_corpus(db: &Database) -> HashMap<String, i64> {
    let now = Utc::now().to_rfc3339();
    let run_id = db.insert_run("eval_semantic", &now).expect("insert run");

    // (source_id, title, summary, vector)
    let rows: Vec<(&str, &str, &str, [f32; 4])> = vec![
        ("sem-seo-1",   "Senior SEO Strategist",    "Keyword research and on-page optimization.", [1.0, 0.1, 0.0, 0.0]),
        ("sem-dev-1",   "Rust Backend Engineer",    "Ship async services with Postgres.",         [0.0, 1.0, 0.1, 0.0]),
        ("sem-bk-1",    "Bookkeeper",               "Quickbooks reconciliation for SMBs.",        [0.0, 0.0, 1.0, 0.0]),
        ("sem-des-1",   "Product Designer",         "Figma prototypes and user testing.",         [0.0, 0.0, 0.0, 1.0]),
        ("sem-hybrid-1","Marketing Engineer",       "Growth ops: SEO analytics + internal tools.", [0.7, 0.6, 0.0, 0.0]),
    ];

    let mut map = HashMap::new();
    for (sid, title, summary, _vec) in &rows {
        let job = Job {
            id: None,
            source: "eval_semantic".to_string(),
            source_id: sid.to_string(),
            title: title.to_string(),
            company: "SemCo".to_string(),
            company_logo_url: String::new(),
            pay: "$1000/mo".to_string(),
            posted_at: now.clone(),
            url: format!("https://example.com/{sid}"),
            summary: summary.to_string(),
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
        db.insert_job(&job, run_id).expect("insert semantic job");
    }

    for j in db.get_jobs(None, false, None).expect("get_jobs") {
        if let Some(id) = j.id {
            map.insert(j.source_id.clone(), id);
        }
    }

    for (sid, _, _, vec) in &rows {
        let job_id = *map.get(*sid).expect("mapped id");
        let vector_json = serde_json::to_string(&vec.to_vec()).expect("vec json");
        db.upsert_job_embedding(job_id, SEMANTIC_MODEL, &vector_json, &now)
            .expect("upsert embedding");
    }

    map
}

#[test]
fn semantic_fallback_ranks_by_cosine_similarity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db = Database::new(tmp.path().to_path_buf()).expect("db");
    let src_to_id = seed_semantic_corpus(&db);

    // (label, query_vector, expected_top_source_id)
    let queries: Vec<(&str, [f32; 4], &str)> = vec![
        ("marketing concepts", [1.0, 0.0, 0.0, 0.0], "sem-seo-1"),
        ("engineering concepts", [0.0, 1.0, 0.0, 0.0], "sem-dev-1"),
        ("finance concepts", [0.0, 0.0, 1.0, 0.0], "sem-bk-1"),
    ];

    let rows = db
        .list_job_embeddings(SEMANTIC_MODEL)
        .expect("list embeddings");
    assert_eq!(rows.len(), 5, "expected 5 seeded embeddings");

    println!("\n=== Semantic Fallback Eval ({} queries) ===", queries.len());
    for (label, qvec, expected_sid) in &queries {
        let candidates = rows.iter().filter_map(|r| {
            let vec: Vec<f32> = serde_json::from_str(&r.vector_json).ok()?;
            Some((r.job_id, vec))
        });
        let ranked = rank_embeddings_against_query(qvec, candidates, &HashSet::new(), 0.30, 5);
        let top = ranked.first().copied().unwrap_or(-1);
        let expected_id = *src_to_id.get(*expected_sid).expect("expected sid mapped");
        let mark = if top == expected_id { "✓" } else { "✗" };
        println!("  {} {:<22} top={:<4}  expected={}", mark, label, top, expected_id);
        assert_eq!(
            top, expected_id,
            "query '{label}' expected top source_id '{expected_sid}' (id={expected_id}), got id={top}"
        );
    }
    println!("===========================================\n");
}

// ─────────────────────────────────────────────────────────────────────────
// Local follow-up resolver goldens
//
// Verifies that the lightweight fast-path in ai::followup correctly
// short-circuits common reference phrasings (without an LLM call) and
// bails out when the message genuinely needs the model.
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn followup_resolver_handles_common_phrasings() {
    let prev_ids: Vec<i64> = vec![201, 202, 203, 204, 205];

    // (label, message, expected outcome)
    #[derive(Debug)]
    enum Expected {
        Select(Vec<i64>),
        Describe(Vec<i64>),
        Fallthrough,
    }

    let cases: Vec<(&str, &str, Expected)> = vec![
        ("first-one",     "show me the first one",          Expected::Select(vec![201])),
        ("second-one",    "just the second one please",     Expected::Select(vec![202])),
        ("3rd-suffix",    "the 3rd one",                    Expected::Select(vec![203])),
        ("last-one",      "give me the last one",           Expected::Select(vec![205])),
        ("first-two",     "show me the first two",          Expected::Select(vec![201, 202])),
        ("top-3",         "just top 3",                     Expected::Select(vec![201, 202, 203])),
        ("all-of-them",   "show me all of them",            Expected::Select(prev_ids.clone())),
        ("those",         "those",                          Expected::Select(prev_ids.clone())),
        ("describe-two",  "describe the first two",         Expected::Describe(vec![201, 202])),
        ("summary-all",   "summary of all of them",         Expected::Describe(prev_ids.clone())),
        ("compare",       "compare the first two",          Expected::Fallthrough),
        ("why-question",  "why is the first one better",    Expected::Fallthrough),
        ("new-search",    "find me rust jobs",              Expected::Fallthrough),
    ];

    println!("\n=== Follow-up Resolver Goldens ({} cases) ===", cases.len());
    let mut failures: Vec<String> = Vec::new();
    for (label, message, expected) in &cases {
        let got = resolve_followup(message, &prev_ids);
        let ok = match (&got, expected) {
            (Some(FollowUpAction::Select(ids)), Expected::Select(want)) => ids == want,
            (Some(FollowUpAction::Describe(ids)), Expected::Describe(want)) => ids == want,
            (None, Expected::Fallthrough) => true,
            _ => false,
        };
        let mark = if ok { "✓" } else { "✗" };
        println!("  {} {:<14}  {:?}", mark, label, got);
        if !ok {
            failures.push(format!("{label}: got {got:?}, wanted {expected:?}"));
        }
    }
    println!("============================================\n");
    assert!(
        failures.is_empty(),
        "follow-up resolver goldens failed: {failures:?}"
    );
}
