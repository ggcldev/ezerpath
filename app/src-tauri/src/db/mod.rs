use crate::ai::{AiConversation, AiMessage, AiRuntimeConfig, EmbeddingIndexStatus, ResumeProfile};
use rusqlite::{Connection, params, params_from_iter};
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Option<i64>,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub company: String,
    pub company_logo_url: String,
    pub pay: String,
    pub posted_at: String,
    pub url: String,
    pub summary: String,
    pub keyword: String,
    pub scraped_at: String,
    pub is_new: bool,
    pub watchlisted: bool,
    pub run_id: Option<i64>,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    pub salary_currency: String,
    pub salary_period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRun {
    pub id: i64,
    pub started_at: String,
    pub keywords: String,
    pub total_found: i64,
    pub total_new: i64,
}

#[derive(Debug, Clone)]
pub struct JobForEmbedding {
    pub id: i64,
    pub title: String,
    pub company: String,
    pub pay: String,
    pub summary: String,
    pub keyword: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct JobEmbeddingRow {
    pub job_id: i64,
    pub title: String,
    pub company: String,
    pub pay: String,
    pub keyword: String,
    pub url: String,
    pub watchlisted: bool,
    pub scraped_at: String,
    pub vector_json: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedPay {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub currency: String,
    pub period: String,
}

pub fn parse_pay(raw: &str) -> ParsedPay {
    let lower = raw.to_lowercase();
    let trimmed = lower.trim();

    if trimmed.is_empty()
        || trimmed == "negotiable"
        || trimmed.starts_with("depend")
        || trimmed.starts_with("to be")
    {
        return ParsedPay::default();
    }

    // Detect currency
    let currency = if trimmed.contains('$') || trimmed.contains("usd") {
        "USD"
    } else if trimmed.contains("php") || trimmed.contains("pesos") || trimmed.contains('₱') {
        "PHP"
    } else {
        ""
    };

    // Detect period
    let period = if trimmed.contains("/hr")
        || trimmed.contains("/hour")
        || trimmed.contains("per hour")
        || trimmed.contains("an hour")
        || trimmed.contains("p/h")
        || trimmed.contains("hourly")
    {
        "hourly"
    } else if trimmed.contains("/mo")
        || trimmed.contains("/month")
        || trimmed.contains("a month")
        || trimmed.contains("monthly")
        || trimmed.contains("/m ")
        || trimmed.ends_with("/m")
    {
        "monthly"
    } else {
        ""
    };

    let cleaned = trimmed.replace(',', "");
    let nums = extract_numbers_from_pay(&cleaned);

    if nums.is_empty() {
        return ParsedPay::default();
    }

    let (min, max) = if nums.len() >= 2 {
        let (a, b) = (nums[0], nums[1]);
        if a <= b { (Some(a), Some(b)) } else { (Some(b), Some(a)) }
    } else {
        (Some(nums[0]), Some(nums[0]))
    };

    // Infer currency from magnitude when not explicitly stated
    let currency = if currency.is_empty() {
        if max.unwrap_or(0.0) > 500.0 { "PHP" } else { "USD" }
    } else {
        currency
    };

    // Infer period from magnitude when not explicitly stated
    let period = if period.is_empty() {
        if max.unwrap_or(0.0) <= 50.0 { "hourly" } else { "monthly" }
    } else {
        period
    };

    ParsedPay {
        min,
        max,
        currency: currency.to_string(),
        period: period.to_string(),
    }
}

fn extract_numbers_from_pay(s: &str) -> Vec<f64> {
    let mut nums = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i].is_ascii_digit() || (chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if let Ok(n) = token.parse::<f64>() {
                if n > 0.0 {
                    nums.push(n);
                }
            }
        } else {
            i += 1;
        }
    }
    nums
}

/// Build a safe FTS5 MATCH expression from arbitrary user text. Strips
/// non-alphanumeric characters from each whitespace-split token, drops empties,
/// then joins with spaces (implicit AND) and appends `*` for prefix matching.
/// Returns an empty string when no usable tokens remain.
pub fn build_fts5_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|tok| tok.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|tok| !tok.is_empty())
        .map(|tok| format!("{tok}*"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    fn conn(&self) -> Result<MutexGuard<'_, Connection>, rusqlite::Error> {
        self.conn
            .lock()
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!("database mutex poisoned: {e}")))))
    }

    pub fn new(app_dir: PathBuf) -> Result<Self, rusqlite::Error> {
        std::fs::create_dir_all(&app_dir).ok();
        let db_path = app_dir.join("ezerpath.db");
        let conn = Connection::open(db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS jobs (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                source      TEXT NOT NULL,
                source_id   TEXT NOT NULL,
                title       TEXT NOT NULL,
                company     TEXT DEFAULT '',
                company_logo_url TEXT DEFAULT '',
                pay         TEXT DEFAULT '',
                posted_at   TEXT DEFAULT '',
                url         TEXT NOT NULL,
                summary     TEXT DEFAULT '',
                keyword     TEXT DEFAULT '',
                scraped_at  TEXT NOT NULL,
                is_new      INTEGER DEFAULT 1,
                watchlisted INTEGER DEFAULT 0,
                UNIQUE(source, source_id)
            );

            CREATE TABLE IF NOT EXISTS keywords (
                id      INTEGER PRIMARY KEY AUTOINCREMENT,
                keyword TEXT NOT NULL UNIQUE
            );

            CREATE TABLE IF NOT EXISTS runs (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at   TEXT NOT NULL,
                keywords     TEXT NOT NULL DEFAULT '',
                status       TEXT NOT NULL DEFAULT 'running',
                error_message TEXT,
                finished_at  TEXT,
                total_found  INTEGER NOT NULL DEFAULT 0,
                total_new    INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS resume_profiles (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL,
                source_file     TEXT NOT NULL DEFAULT '',
                raw_text        TEXT NOT NULL DEFAULT '',
                normalized_text TEXT NOT NULL DEFAULT '',
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                is_active       INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS job_embeddings (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id       INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
                model_name   TEXT NOT NULL,
                vector       TEXT NOT NULL DEFAULT '',
                updated_at   TEXT NOT NULL,
                UNIQUE(job_id, model_name)
            );

            CREATE TABLE IF NOT EXISTS resume_embeddings (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                resume_id    INTEGER NOT NULL REFERENCES resume_profiles(id) ON DELETE CASCADE,
                model_name   TEXT NOT NULL,
                vector       TEXT NOT NULL DEFAULT '',
                updated_at   TEXT NOT NULL,
                UNIQUE(resume_id, model_name)
            );

            CREATE TABLE IF NOT EXISTS ai_conversations (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                title       TEXT NOT NULL DEFAULT 'New Chat',
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ai_messages (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id  INTEGER NOT NULL REFERENCES ai_conversations(id) ON DELETE CASCADE,
                role             TEXT NOT NULL,
                content          TEXT NOT NULL,
                created_at       TEXT NOT NULL,
                meta_json        TEXT NOT NULL DEFAULT '{}'
            );

            CREATE TABLE IF NOT EXISTS ai_runs (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                task_type   TEXT NOT NULL,
                latency_ms  INTEGER NOT NULL DEFAULT 0,
                status      TEXT NOT NULL,
                error       TEXT,
                created_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                key         TEXT PRIMARY KEY,
                value       TEXT NOT NULL
            );

            INSERT OR IGNORE INTO keywords (keyword) VALUES ('seo specialist');
            INSERT OR IGNORE INTO keywords (keyword) VALUES ('link building');
            INSERT OR IGNORE INTO keywords (keyword) VALUES ('outreach');
            INSERT OR IGNORE INTO keywords (keyword) VALUES ('content writer');
            "
        )?;

        // Migration: add run_id to jobs if not yet present (ignore error if already exists)
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN run_id INTEGER REFERENCES runs(id);").ok();
        // Migration: add company logo URL if not yet present.
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN company_logo_url TEXT DEFAULT '';").ok();
        // Migration: add run lifecycle fields if not yet present.
        conn.execute_batch("ALTER TABLE runs ADD COLUMN status TEXT NOT NULL DEFAULT 'succeeded';").ok();
        conn.execute_batch("ALTER TABLE runs ADD COLUMN error_message TEXT;").ok();
        conn.execute_batch("ALTER TABLE runs ADD COLUMN finished_at TEXT;").ok();
        // Migration: add normalized salary fields.
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN salary_min REAL;").ok();
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN salary_max REAL;").ok();
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN salary_currency TEXT DEFAULT '';").ok();
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN salary_period TEXT DEFAULT '';").ok();
        // Migration: add linked job IDs for chat follow-up context.
        conn.execute_batch("ALTER TABLE ai_messages ADD COLUMN linked_job_ids_json TEXT DEFAULT '[]';").ok();
        // Migration: telemetry breakdown on ai_runs (phase #2).
        conn.execute_batch("ALTER TABLE ai_runs ADD COLUMN intent TEXT;").ok();
        conn.execute_batch("ALTER TABLE ai_runs ADD COLUMN route TEXT;").ok();
        conn.execute_batch("ALTER TABLE ai_runs ADD COLUMN candidate_job_ids TEXT;").ok();
        conn.execute_batch("ALTER TABLE ai_runs ADD COLUMN final_job_ids TEXT;").ok();
        conn.execute_batch("ALTER TABLE ai_runs ADD COLUMN retrieval_ms INTEGER;").ok();
        conn.execute_batch("ALTER TABLE ai_runs ADD COLUMN llm_ms INTEGER;").ok();

        // Migration: FTS5 index over jobs.title/company/summary (phase #3).
        // Uses contentless-mirror pattern: jobs_fts is a shadow table kept in
        // sync via triggers, queryable with bm25() ranking.
        let fts_create = conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS jobs_fts USING fts5(
                title, company, summary,
                content='jobs', content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS jobs_ai AFTER INSERT ON jobs BEGIN
                INSERT INTO jobs_fts(rowid, title, company, summary)
                VALUES (new.id, new.title, new.company, new.summary);
            END;

            CREATE TRIGGER IF NOT EXISTS jobs_ad AFTER DELETE ON jobs BEGIN
                INSERT INTO jobs_fts(jobs_fts, rowid, title, company, summary)
                VALUES('delete', old.id, old.title, old.company, old.summary);
            END;

            CREATE TRIGGER IF NOT EXISTS jobs_au AFTER UPDATE ON jobs BEGIN
                INSERT INTO jobs_fts(jobs_fts, rowid, title, company, summary)
                VALUES('delete', old.id, old.title, old.company, old.summary);
                INSERT INTO jobs_fts(rowid, title, company, summary)
                VALUES (new.id, new.title, new.company, new.summary);
            END;",
        );
        // Backfill from jobs if the FTS table was just created or is empty,
        // and rebuild whenever the FTS5 index fails its own integrity check.
        // Without this, a corrupt FTS5 shadow index causes any subsequent
        // UPDATE on `jobs` (e.g. the salary backfill below) to fail with
        // "database disk image is malformed" via the jobs_au trigger,
        // which prevents the app from starting.
        if fts_create.is_ok() {
            let count: i64 = conn
                .query_row("SELECT count(*) FROM jobs_fts", [], |r| r.get(0))
                .unwrap_or(0);
            let fts_ok = conn
                .execute_batch("INSERT INTO jobs_fts(jobs_fts) VALUES('integrity-check');")
                .is_ok();
            if count == 0 || !fts_ok {
                conn.execute_batch("INSERT INTO jobs_fts(jobs_fts) VALUES('rebuild');").ok();
            }
        }

        // Backfill salary fields for existing rows that have a pay string but no parsed salary.
        {
            let mut stmt = conn.prepare(
                "SELECT id, pay FROM jobs WHERE pay != '' AND salary_max IS NULL",
            )?;
            let rows: Vec<(i64, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            for (id, pay_str) in rows {
                let parsed = parse_pay(&pay_str);
                conn.execute(
                    "UPDATE jobs SET salary_min = ?1, salary_max = ?2, salary_currency = ?3, salary_period = ?4 WHERE id = ?5",
                    params![parsed.min, parsed.max, parsed.currency, parsed.period, id],
                )?;
            }
        }

        let defaults = AiRuntimeConfig::default();
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('ai_ollama_base_url', ?1)",
            params![defaults.ollama_base_url],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('ai_ollama_model', ?1)",
            params![defaults.ollama_model],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('ai_embedding_service_url', ?1)",
            params![defaults.embedding_service_url],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('ai_embedding_model', ?1)",
            params![defaults.embedding_model],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('ai_temperature', ?1)",
            params![defaults.temperature.to_string()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('ai_max_tokens', ?1)",
            params![defaults.max_tokens.to_string()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('ai_timeout_ms', ?1)",
            params![defaults.timeout_ms.to_string()],
        )?;

        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn insert_run(&self, keywords: &str, started_at: &str) -> Result<i64, rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO runs (started_at, keywords, status) VALUES (?1, ?2, 'running')",
            params![started_at, keywords],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn complete_run(&self, run_id: i64, total_found: i64, total_new: i64, finished_at: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE runs
             SET total_found = ?1, total_new = ?2, status = 'succeeded', error_message = NULL, finished_at = ?3
             WHERE id = ?4",
            params![total_found, total_new, finished_at, run_id],
        )?;
        Ok(())
    }

    pub fn fail_run(&self, run_id: i64, total_found: i64, total_new: i64, error_message: &str, finished_at: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE runs
             SET total_found = ?1, total_new = ?2, status = 'failed', error_message = ?3, finished_at = ?4
             WHERE id = ?5",
            params![total_found, total_new, error_message, finished_at, run_id],
        )?;
        Ok(())
    }

    pub fn get_runs(&self) -> Result<Vec<ScanRun>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, started_at, keywords, total_found, total_new FROM runs ORDER BY started_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ScanRun {
                id: row.get(0)?,
                started_at: row.get(1)?,
                keywords: row.get(2)?,
                total_found: row.get(3)?,
                total_new: row.get(4)?,
            })
        })?;
        let mut runs = Vec::new();
        for row in rows { runs.push(row?); }
        Ok(runs)
    }

    pub fn delete_run(&self, run_id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM jobs WHERE run_id = ?1", params![run_id])?;
        conn.execute("DELETE FROM runs WHERE id = ?1", params![run_id])?;
        Ok(())
    }

    pub fn clear_all_jobs(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute_batch("DELETE FROM jobs; DELETE FROM runs;")?;
        Ok(())
    }

    pub fn insert_job(&self, job: &Job, run_id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn()?;
        let parsed = parse_pay(&job.pay);
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO jobs (source, source_id, title, company, company_logo_url, pay, posted_at, url, summary, keyword, scraped_at, is_new, run_id, salary_min, salary_max, salary_currency, salary_period)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                job.source, job.source_id, job.title, job.company, job.company_logo_url, job.pay,
                job.posted_at, job.url, job.summary, job.keyword, job.scraped_at,
                job.is_new as i32, run_id,
                parsed.min, parsed.max, parsed.currency, parsed.period,
            ],
        )?;

        if inserted > 0 {
            return Ok(true);
        }

        // Existing job found again in a newer scan: refresh key fields and attach to latest run.
        // scraped_at is intentionally NOT updated here — it records when the job was first seen.
        // Refreshing it would cause old jobs to pass the daysAgo filter incorrectly.
        conn.execute(
            "UPDATE jobs
             SET title = ?1,
                 company = ?2,
                 company_logo_url = ?3,
                 pay = ?4,
                 posted_at = ?5,
                 url = ?6,
                 summary = ?7,
                 keyword = ?8,
                 is_new = 0,
                 run_id = ?9,
                 salary_min = ?12,
                 salary_max = ?13,
                 salary_currency = ?14,
                 salary_period = ?15
             WHERE source = ?10 AND source_id = ?11",
            params![
                job.title,
                job.company,
                job.company_logo_url,
                job.pay,
                job.posted_at,
                job.url,
                job.summary,
                job.keyword,
                run_id,
                job.source,
                job.source_id,
                parsed.min,
                parsed.max,
                parsed.currency,
                parsed.period,
            ],
        )?;

        Ok(false)
    }

    pub fn get_jobs(&self, keyword: Option<&str>, watchlisted_only: bool, days_ago: Option<i64>) -> Result<Vec<Job>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut query = String::from(
            "SELECT id, source, source_id, title, company, company_logo_url, pay, posted_at, url, summary, keyword, scraped_at, is_new, watchlisted, run_id, salary_min, salary_max, salary_currency, salary_period
             FROM jobs WHERE 1=1"
        );
        let mut bind_values: Vec<Value> = Vec::new();

        if watchlisted_only {
            query.push_str(" AND watchlisted = 1");
        }
        if let Some(days) = days_ago {
            let bounded_days = days.clamp(0, 3650);
            query.push_str(" AND julianday(scraped_at) >= julianday('now', ?)");
            bind_values.push(Value::Text(format!("-{bounded_days} days")));
        }
        if let Some(kw) = keyword {
            query.push_str(" AND keyword = ?");
            bind_values.push(Value::Text(kw.to_string()));
        }
        query.push_str(" ORDER BY scraped_at DESC");

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(bind_values.iter()), row_to_job)?;

        let mut jobs = Vec::new();
        for row in rows { jobs.push(row?); }
        Ok(jobs)
    }

    /// Full-text search over jobs (title, company, summary) ranked by bm25.
    /// Tokens are extracted from the user query, sanitized, and joined with
    /// implicit AND. Each token gets a `*` prefix so partial matches work.
    pub fn search_jobs_fts(&self, query: &str, limit: usize) -> Result<Vec<Job>, rusqlite::Error> {
        let fts_query = build_fts5_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let sql = "SELECT j.id, j.source, j.source_id, j.title, j.company, j.company_logo_url,
                          j.pay, j.posted_at, j.url, j.summary, j.keyword, j.scraped_at,
                          j.is_new, j.watchlisted, j.run_id, j.salary_min, j.salary_max,
                          j.salary_currency, j.salary_period
                   FROM jobs_fts
                   JOIN jobs j ON j.id = jobs_fts.rowid
                   WHERE jobs_fts MATCH ?1
                   ORDER BY bm25(jobs_fts)
                   LIMIT ?2";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![fts_query, limit as i64], row_to_job)?;
        let mut jobs = Vec::new();
        for row in rows { jobs.push(row?); }
        Ok(jobs)
    }

    pub fn get_jobs_by_ids(&self, ids: &[i64]) -> Result<Vec<Job>, rusqlite::Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let order_cases: Vec<String> = ids
            .iter()
            .enumerate()
            .map(|(idx, _)| format!("WHEN ? THEN {}", idx))
            .collect();
        let query = format!(
            "SELECT id, source, source_id, title, company, company_logo_url, pay, posted_at, url, summary, keyword, scraped_at, is_new, watchlisted, run_id, salary_min, salary_max, salary_currency, salary_period
             FROM jobs WHERE id IN ({})
             ORDER BY CASE id {} ELSE {} END",
            placeholders.join(","),
            order_cases.join(" "),
            ids.len()
        );
        let mut bind_values: Vec<Value> = ids.iter().map(|id| Value::Integer(*id)).collect();
        bind_values.extend(ids.iter().map(|id| Value::Integer(*id)));
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(bind_values.iter()), row_to_job)?;
        let mut jobs = Vec::new();
        for row in rows { jobs.push(row?); }
        Ok(jobs)
    }

    pub fn get_top_paying_jobs(&self, keyword_filter: Option<&str>, title_terms: &[String], limit: usize) -> Result<Vec<Job>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut query = String::from(
            "SELECT id, source, source_id, title, company, company_logo_url, pay, posted_at, url, summary, keyword, scraped_at, is_new, watchlisted, run_id, salary_min, salary_max, salary_currency, salary_period
             FROM jobs WHERE salary_min IS NOT NULL"
        );
        let mut bind_values: Vec<Value> = Vec::new();
        if let Some(kw) = keyword_filter {
            query.push_str(" AND keyword = ?");
            bind_values.push(Value::Text(kw.to_string()));
        }
        if !title_terms.is_empty() {
            let clauses: Vec<String> = title_terms.iter().map(|_| "LOWER(title) LIKE ?".to_string()).collect();
            query.push_str(&format!(" AND ({})", clauses.join(" OR ")));
            for t in title_terms {
                bind_values.push(Value::Text(format!("%{}%", t.to_lowercase())));
            }
        }
        // Ranking normalization:
        // - Convert hourly rates to monthly equivalent (160h / month)
        // - Convert PHP to approximate USD for cross-currency ordering
        query.push_str(
            " ORDER BY
             CASE UPPER(COALESCE(salary_currency, ''))
                 WHEN 'PHP' THEN
                     (CASE LOWER(COALESCE(salary_period, ''))
                         WHEN 'hourly' THEN salary_min * 160.0
                         WHEN 'monthly' THEN salary_min
                         ELSE salary_min
                     END) / 55.0
                 WHEN 'USD' THEN
                     CASE LOWER(COALESCE(salary_period, ''))
                         WHEN 'hourly' THEN salary_min * 160.0
                         WHEN 'monthly' THEN salary_min
                         ELSE salary_min
                     END
                 ELSE
                     CASE LOWER(COALESCE(salary_period, ''))
                         WHEN 'hourly' THEN salary_min * 160.0
                         WHEN 'monthly' THEN salary_min
                         ELSE salary_min
                     END
             END DESC,
             salary_min DESC
             LIMIT ?"
        );
        bind_values.push(Value::Integer(limit as i64));
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(bind_values.iter()), row_to_job)?;
        let mut jobs = Vec::new();
        for row in rows { jobs.push(row?); }
        Ok(jobs)
    }

    pub fn toggle_watchlist(&self, job_id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE jobs SET watchlisted = CASE WHEN watchlisted = 1 THEN 0 ELSE 1 END WHERE id = ?1",
            params![job_id],
        )?;
        let watchlisted: bool = conn.query_row(
            "SELECT watchlisted FROM jobs WHERE id = ?1",
            params![job_id],
            |row| row.get(0),
        )?;
        Ok(watchlisted)
    }

    pub fn get_keywords(&self) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT keyword FROM keywords ORDER BY keyword")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut keywords = Vec::new();
        for row in rows { keywords.push(row?); }
        Ok(keywords)
    }

    pub fn add_keyword(&self, keyword: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute("INSERT OR IGNORE INTO keywords (keyword) VALUES (?1)", params![keyword])?;
        Ok(())
    }

    pub fn remove_keyword(&self, keyword: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM keywords WHERE keyword = ?1", params![keyword])?;
        Ok(())
    }

    pub fn save_resume_profile(&self, name: &str, source_file: Option<&str>, raw_text: &str, normalized_text: &str, now: &str) -> Result<ResumeProfile, rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute("UPDATE resume_profiles SET is_active = 0", [])?;
        conn.execute(
            "INSERT INTO resume_profiles (name, source_file, raw_text, normalized_text, created_at, updated_at, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            params![name, source_file.unwrap_or(""), raw_text, normalized_text, now, now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(ResumeProfile {
            id,
            name: name.to_string(),
            source_file: source_file.unwrap_or("").to_string(),
            raw_text: raw_text.to_string(),
            normalized_text: normalized_text.to_string(),
            created_at: now.to_string(),
            updated_at: now.to_string(),
            is_active: true,
        })
    }

    pub fn list_resume_profiles(&self) -> Result<Vec<ResumeProfile>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, source_file, raw_text, normalized_text, created_at, updated_at, is_active
             FROM resume_profiles
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ResumeProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                source_file: row.get(2)?,
                raw_text: row.get(3)?,
                normalized_text: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                is_active: row.get::<_, i32>(7)? != 0,
            })
        })?;
        let mut out = Vec::new();
        for row in rows { out.push(row?); }
        Ok(out)
    }

    pub fn set_active_resume(&self, resume_id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute("UPDATE resume_profiles SET is_active = 0", [])?;
        conn.execute(
            "UPDATE resume_profiles SET is_active = 1, updated_at = ?1 WHERE id = ?2",
            params![chrono::Utc::now().to_rfc3339(), resume_id],
        )?;
        Ok(())
    }

    pub fn get_resume_profile(&self, resume_id: i64) -> Result<Option<ResumeProfile>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, source_file, raw_text, normalized_text, created_at, updated_at, is_active
             FROM resume_profiles WHERE id = ?1 LIMIT 1",
        )?;
        let mut rows = stmt.query(params![resume_id])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(ResumeProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                source_file: row.get(2)?,
                raw_text: row.get(3)?,
                normalized_text: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                is_active: row.get::<_, i32>(7)? != 0,
            }));
        }
        Ok(None)
    }

    pub fn list_jobs_for_embedding(&self) -> Result<Vec<JobForEmbedding>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, company, pay, summary, keyword, url
             FROM jobs
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(JobForEmbedding {
                id: row.get(0)?,
                title: row.get(1)?,
                company: row.get(2)?,
                pay: row.get(3)?,
                summary: row.get(4)?,
                keyword: row.get(5)?,
                url: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows { out.push(row?); }
        Ok(out)
    }

    pub fn upsert_job_embedding(&self, job_id: i64, model_name: &str, vector_json: &str, updated_at: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO job_embeddings (job_id, model_name, vector, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(job_id, model_name)
             DO UPDATE SET vector = excluded.vector, updated_at = excluded.updated_at",
            params![job_id, model_name, vector_json, updated_at],
        )?;
        Ok(())
    }

    pub fn upsert_resume_embedding(&self, resume_id: i64, model_name: &str, vector_json: &str, updated_at: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO resume_embeddings (resume_id, model_name, vector, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(resume_id, model_name)
             DO UPDATE SET vector = excluded.vector, updated_at = excluded.updated_at",
            params![resume_id, model_name, vector_json, updated_at],
        )?;
        Ok(())
    }

    pub fn get_resume_embedding(&self, resume_id: i64, model_name: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT vector FROM resume_embeddings WHERE resume_id = ?1 AND model_name = ?2 LIMIT 1",
        )?;
        let mut rows = stmt.query(params![resume_id, model_name])?;
        if let Some(row) = rows.next()? {
            let vector: String = row.get(0)?;
            return Ok(Some(vector));
        }
        Ok(None)
    }

    pub fn list_job_embeddings(&self, model_name: &str) -> Result<Vec<JobEmbeddingRow>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT e.job_id, j.title, j.company, j.pay, j.keyword, j.url, j.watchlisted, j.scraped_at, e.vector
             FROM job_embeddings e
             JOIN jobs j ON j.id = e.job_id
             WHERE e.model_name = ?1",
        )?;
        let rows = stmt.query_map(params![model_name], |row| {
            Ok(JobEmbeddingRow {
                job_id: row.get(0)?,
                title: row.get(1)?,
                company: row.get(2)?,
                pay: row.get(3)?,
                keyword: row.get(4)?,
                url: row.get(5)?,
                watchlisted: row.get::<_, i32>(6)? != 0,
                scraped_at: row.get(7)?,
                vector_json: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows { out.push(row?); }
        Ok(out)
    }

    pub fn create_ai_conversation(&self, title: Option<&str>, now: &str) -> Result<AiConversation, rusqlite::Error> {
        let conn = self.conn()?;
        let title = title.unwrap_or("New Chat");
        conn.execute(
            "INSERT INTO ai_conversations (title, created_at, updated_at) VALUES (?1, ?2, ?3)",
            params![title, now, now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(AiConversation {
            id,
            title: title.to_string(),
            created_at: now.to_string(),
            updated_at: now.to_string(),
        })
    }

    pub fn maybe_set_ai_conversation_title(&self, conversation_id: i64, title: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE ai_conversations
             SET title = ?1
             WHERE id = ?2
               AND (title = 'New Chat' OR title = 'Job Copilot Chat' OR TRIM(title) = '')",
            params![title, conversation_id],
        )?;
        Ok(())
    }

    pub fn list_ai_conversations(&self) -> Result<Vec<AiConversation>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, created_at, updated_at FROM ai_conversations ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AiConversation {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows { out.push(row?); }
        Ok(out)
    }

    pub fn append_ai_message(&self, conversation_id: i64, role: &str, content: &str, meta_json: &str, linked_job_ids: &[i64], now: &str) -> Result<AiMessage, rusqlite::Error> {
        let conn = self.conn()?;
        let linked_json = serde_json::to_string(linked_job_ids).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO ai_messages (conversation_id, role, content, created_at, meta_json, linked_job_ids_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![conversation_id, role, content, now, meta_json, linked_json],
        )?;
        conn.execute(
            "UPDATE ai_conversations SET updated_at = ?1 WHERE id = ?2",
            params![now, conversation_id],
        )?;
        let id = conn.last_insert_rowid();
        Ok(AiMessage {
            id,
            conversation_id,
            role: role.to_string(),
            content: content.to_string(),
            created_at: now.to_string(),
            meta_json: meta_json.to_string(),
            linked_job_ids_json: linked_json,
        })
    }

    pub fn get_ai_messages(&self, conversation_id: i64) -> Result<Vec<AiMessage>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, created_at, meta_json, linked_job_ids_json
             FROM ai_messages
             WHERE conversation_id = ?1
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(AiMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
                meta_json: row.get(5)?,
                linked_job_ids_json: row.get::<_, String>(6).unwrap_or_else(|_| "[]".to_string()),
            })
        })?;
        let mut out = Vec::new();
        for row in rows { out.push(row?); }
        Ok(out)
    }

    pub fn delete_ai_conversation(&self, conversation_id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM ai_messages WHERE conversation_id = ?1", params![conversation_id])?;
        conn.execute("DELETE FROM ai_conversations WHERE id = ?1", params![conversation_id])?;
        Ok(())
    }

    pub fn clear_ai_conversations(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute_batch("DELETE FROM ai_messages; DELETE FROM ai_conversations;")?;
        Ok(())
    }

    pub fn embedding_index_status(&self, embedding_model: &str) -> Result<EmbeddingIndexStatus, rusqlite::Error> {
        let conn = self.conn()?;
        let jobs_total: i64 = conn.query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))?;
        let resumes_total: i64 = conn.query_row("SELECT COUNT(*) FROM resume_profiles", [], |row| row.get(0))?;
        let jobs_indexed: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT job_id) FROM job_embeddings WHERE model_name = ?1",
            params![embedding_model],
            |row| row.get(0),
        )?;
        let resumes_indexed: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT resume_id) FROM resume_embeddings WHERE model_name = ?1",
            params![embedding_model],
            |row| row.get(0),
        )?;
        Ok(EmbeddingIndexStatus {
            jobs_total,
            jobs_indexed,
            resumes_total,
            resumes_indexed,
            active_embedding_model: embedding_model.to_string(),
        })
    }

    pub fn get_ai_runtime_config(&self) -> Result<AiRuntimeConfig, rusqlite::Error> {
        let conn = self.conn()?;
        let get = |key: &str| -> Result<String, rusqlite::Error> {
            conn.query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
        };
        let default = AiRuntimeConfig::default();
        Ok(AiRuntimeConfig {
            ollama_base_url: get("ai_ollama_base_url").unwrap_or(default.ollama_base_url),
            ollama_model: get("ai_ollama_model").unwrap_or(default.ollama_model),
            embedding_service_url: get("ai_embedding_service_url").unwrap_or(default.embedding_service_url),
            embedding_model: get("ai_embedding_model").unwrap_or(default.embedding_model),
            temperature: get("ai_temperature").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(default.temperature),
            max_tokens: get("ai_max_tokens").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(default.max_tokens),
            timeout_ms: get("ai_timeout_ms").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(default.timeout_ms),
        })
    }

    pub fn set_ai_runtime_config(&self, cfg: &AiRuntimeConfig) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        let upsert = |key: &str, value: String| -> Result<(), rusqlite::Error> {
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        };
        upsert("ai_ollama_base_url", cfg.ollama_base_url.clone())?;
        upsert("ai_ollama_model", cfg.ollama_model.clone())?;
        upsert("ai_embedding_service_url", cfg.embedding_service_url.clone())?;
        upsert("ai_embedding_model", cfg.embedding_model.clone())?;
        upsert("ai_temperature", cfg.temperature.to_string())?;
        upsert("ai_max_tokens", cfg.max_tokens.to_string())?;
        upsert("ai_timeout_ms", cfg.timeout_ms.to_string())?;
        Ok(())
    }

    pub fn log_ai_run(&self, log: &AiRunLog) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        let candidates_json = log
            .candidate_job_ids
            .map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string()));
        let finals_json = log
            .final_job_ids
            .map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".to_string()));
        conn.execute(
            "INSERT INTO ai_runs (task_type, latency_ms, status, error, created_at,
                                  intent, route, candidate_job_ids, final_job_ids,
                                  retrieval_ms, llm_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                log.task_type,
                log.latency_ms,
                log.status,
                log.error.unwrap_or(""),
                log.created_at,
                log.intent,
                log.route,
                candidates_json,
                finals_json,
                log.retrieval_ms,
                log.llm_ms,
            ],
        )?;
        Ok(())
    }
}

#[derive(Default)]
pub struct AiRunLog<'a> {
    pub task_type: &'a str,
    pub latency_ms: i64,
    pub status: &'a str,
    pub error: Option<&'a str>,
    pub created_at: &'a str,
    pub intent: Option<&'a str>,
    pub route: Option<&'a str>,
    pub candidate_job_ids: Option<&'a [i64]>,
    pub final_job_ids: Option<&'a [i64]>,
    pub retrieval_ms: Option<i64>,
    pub llm_ms: Option<i64>,
}

fn row_to_job(row: &rusqlite::Row) -> Result<Job, rusqlite::Error> {
    Ok(Job {
        id: Some(row.get(0)?),
        source: row.get(1)?,
        source_id: row.get(2)?,
        title: row.get(3)?,
        company: row.get(4)?,
        company_logo_url: row.get(5)?,
        pay: row.get(6)?,
        posted_at: row.get(7)?,
        url: row.get(8)?,
        summary: row.get(9)?,
        keyword: row.get(10)?,
        scraped_at: row.get(11)?,
        is_new: row.get::<_, i32>(12)? != 0,
        watchlisted: row.get::<_, i32>(13)? != 0,
        run_id: row.get(14)?,
        salary_min: row.get(15)?,
        salary_max: row.get(16)?,
        salary_currency: row.get::<_, String>(17).unwrap_or_default(),
        salary_period: row.get::<_, String>(18).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::{build_fts5_query, Database, Job};
    use chrono::{Duration, Utc};
    use tempfile::tempdir;

    fn mk_job(source_id: &str, scraped_at: String) -> Job {
        Job {
            id: None,
            source: "onlinejobs".to_string(),
            source_id: source_id.to_string(),
            title: "SEO Specialist".to_string(),
            company: "Acme".to_string(),
            company_logo_url: "https://www.onlinejobs.ph/example-logo.png".to_string(),
            pay: "$8/hr".to_string(),
            posted_at: "2026-04-10T00:00:00Z".to_string(),
            url: format!("https://www.onlinejobs.ph/jobseekers/job/{source_id}"),
            summary: "summary".to_string(),
            keyword: "seo specialist".to_string(),
            scraped_at,
            is_new: true,
            watchlisted: false,
            run_id: None,
            salary_min: None,
            salary_max: None,
            salary_currency: String::new(),
            salary_period: String::new(),
        }
    }

    #[test]
    fn duplicate_job_updates_to_latest_run_id() {
        let tmp = tempdir().expect("tempdir");
        let db = Database::new(tmp.path().to_path_buf()).expect("db");

        let run1 = db
            .insert_run("seo specialist", &Utc::now().to_rfc3339())
            .expect("run1");
        let inserted_first = db
            .insert_job(&mk_job("123", Utc::now().to_rfc3339()), run1)
            .expect("insert first");
        assert!(inserted_first);

        let run2 = db
            .insert_run("seo specialist", &Utc::now().to_rfc3339())
            .expect("run2");
        let inserted_second = db
            .insert_job(&mk_job("123", Utc::now().to_rfc3339()), run2)
            .expect("insert second");
        assert!(!inserted_second);

        let jobs = db.get_jobs(None, false, None).expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].run_id, Some(run2));
        assert!(!jobs[0].is_new);
    }

    #[test]
    fn days_filter_excludes_old_rows_with_julianday_comparison() {
        let tmp = tempdir().expect("tempdir");
        let db = Database::new(tmp.path().to_path_buf()).expect("db");
        let run = db
            .insert_run("seo specialist", &Utc::now().to_rfc3339())
            .expect("run");

        let recent = mk_job("recent", Utc::now().to_rfc3339());
        let old = mk_job("old", (Utc::now() - Duration::days(5)).to_rfc3339());
        db.insert_job(&recent, run).expect("insert recent");
        db.insert_job(&old, run).expect("insert old");

        let filtered = db.get_jobs(None, false, Some(1)).expect("filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source_id, "recent");
    }

    #[test]
    fn get_jobs_by_ids_preserves_input_order() {
        let tmp = tempdir().expect("tempdir");
        let db = Database::new(tmp.path().to_path_buf()).expect("db");
        let run = db
            .insert_run("seo specialist", &Utc::now().to_rfc3339())
            .expect("run");

        let mut a = mk_job("one", (Utc::now() - Duration::days(2)).to_rfc3339());
        a.title = "Job One".to_string();
        db.insert_job(&a, run).expect("insert one");

        let mut b = mk_job("two", Utc::now().to_rfc3339());
        b.title = "Job Two".to_string();
        db.insert_job(&b, run).expect("insert two");

        let mut c = mk_job("three", (Utc::now() - Duration::days(1)).to_rfc3339());
        c.title = "Job Three".to_string();
        db.insert_job(&c, run).expect("insert three");

        let jobs = db.get_jobs(None, false, None).expect("jobs");
        let id_one = jobs
            .iter()
            .find(|j| j.source_id == "one")
            .and_then(|j| j.id)
            .expect("id one");
        let id_two = jobs
            .iter()
            .find(|j| j.source_id == "two")
            .and_then(|j| j.id)
            .expect("id two");
        let id_three = jobs
            .iter()
            .find(|j| j.source_id == "three")
            .and_then(|j| j.id)
            .expect("id three");

        let ordered = db
            .get_jobs_by_ids(&[id_two, id_one, id_three])
            .expect("ordered jobs");
        let ordered_ids: Vec<i64> = ordered.into_iter().filter_map(|j| j.id).collect();
        assert_eq!(ordered_ids, vec![id_two, id_one, id_three]);
    }

    #[test]
    fn fts5_query_strips_punctuation_and_adds_prefix() {
        assert_eq!(build_fts5_query("seo outreach"), "seo* outreach*");
        assert_eq!(build_fts5_query("link-building!"), "linkbuilding*");
        assert_eq!(build_fts5_query("  "), "");
        assert_eq!(build_fts5_query("c++ dev"), "c* dev*");
    }

    #[test]
    fn search_jobs_fts_ranks_title_matches_above_summary_only() {
        let tmp = tempdir().expect("tempdir");
        let db = Database::new(tmp.path().to_path_buf()).expect("db");
        let run = db
            .insert_run("seo specialist", &Utc::now().to_rfc3339())
            .expect("run");

        let mut title_hit = mk_job("title_hit", Utc::now().to_rfc3339());
        title_hit.title = "Link Building Outreach Specialist".to_string();
        title_hit.summary = "Help with content.".to_string();
        db.insert_job(&title_hit, run).expect("insert title hit");

        let mut summary_only = mk_job("summary_only", Utc::now().to_rfc3339());
        summary_only.title = "Content Writer".to_string();
        summary_only.summary = "Some link building knowledge needed.".to_string();
        db.insert_job(&summary_only, run).expect("insert summary only");

        let mut unrelated = mk_job("unrelated", Utc::now().to_rfc3339());
        unrelated.title = "Bookkeeper".to_string();
        unrelated.summary = "Quickbooks experience.".to_string();
        db.insert_job(&unrelated, run).expect("insert unrelated");

        let results = db.search_jobs_fts("link building", 10).expect("fts");
        let titles: Vec<&str> = results.iter().map(|j| j.title.as_str()).collect();
        assert_eq!(titles.len(), 2);
        assert_eq!(titles[0], "Link Building Outreach Specialist");
    }

    #[test]
    fn top_paying_jobs_ranking_normalizes_currency_and_period() {
        let tmp = tempdir().expect("tempdir");
        let db = Database::new(tmp.path().to_path_buf()).expect("db");
        let run = db
            .insert_run("seo specialist", &Utc::now().to_rfc3339())
            .expect("run");

        let mut hourly_usd = mk_job("hourly_usd", Utc::now().to_rfc3339());
        hourly_usd.title = "Hourly USD".to_string();
        hourly_usd.pay = "$10/hr".to_string(); // ~1600 USD/mo
        db.insert_job(&hourly_usd, run).expect("insert hourly");

        let mut monthly_usd = mk_job("monthly_usd", Utc::now().to_rfc3339());
        monthly_usd.title = "Monthly USD".to_string();
        monthly_usd.pay = "$1500/mo".to_string(); // 1500 USD/mo
        db.insert_job(&monthly_usd, run).expect("insert monthly usd");

        let mut monthly_php = mk_job("monthly_php", Utc::now().to_rfc3339());
        monthly_php.title = "Monthly PHP".to_string();
        monthly_php.pay = "₱55,000/mo".to_string(); // ~1000 USD/mo at /55
        db.insert_job(&monthly_php, run).expect("insert monthly php");

        let ranked = db
            .get_top_paying_jobs(None, &[], 3)
            .expect("ranked jobs");
        let titles: Vec<String> = ranked.iter().map(|j| j.title.clone()).collect();
        assert_eq!(titles, vec!["Hourly USD", "Monthly USD", "Monthly PHP"]);
    }
}
