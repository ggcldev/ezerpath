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
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO jobs (source, source_id, title, company, company_logo_url, pay, posted_at, url, summary, keyword, scraped_at, is_new, run_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                job.source, job.source_id, job.title, job.company, job.company_logo_url, job.pay,
                job.posted_at, job.url, job.summary, job.keyword, job.scraped_at,
                job.is_new as i32, run_id,
            ],
        )?;

        if inserted > 0 {
            return Ok(true);
        }

        // Existing job found again in a newer scan: refresh key fields and attach to latest run.
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
                 scraped_at = ?9,
                 is_new = 0,
                 run_id = ?10
             WHERE source = ?11 AND source_id = ?12",
            params![
                job.title,
                job.company,
                job.company_logo_url,
                job.pay,
                job.posted_at,
                job.url,
                job.summary,
                job.keyword,
                job.scraped_at,
                run_id,
                job.source,
                job.source_id,
            ],
        )?;

        Ok(false)
    }

    pub fn get_jobs(&self, keyword: Option<&str>, watchlisted_only: bool, days_ago: Option<i64>) -> Result<Vec<Job>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut query = String::from(
            "SELECT id, source, source_id, title, company, company_logo_url, pay, posted_at, url, summary, keyword, scraped_at, is_new, watchlisted, run_id
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

    pub fn append_ai_message(&self, conversation_id: i64, role: &str, content: &str, meta_json: &str, now: &str) -> Result<AiMessage, rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO ai_messages (conversation_id, role, content, created_at, meta_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![conversation_id, role, content, now, meta_json],
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
        })
    }

    pub fn get_ai_messages(&self, conversation_id: i64) -> Result<Vec<AiMessage>, rusqlite::Error> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, created_at, meta_json
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
            })
        })?;
        let mut out = Vec::new();
        for row in rows { out.push(row?); }
        Ok(out)
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

    pub fn log_ai_run(&self, task_type: &str, latency_ms: i64, status: &str, error: Option<&str>, created_at: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO ai_runs (task_type, latency_ms, status, error, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_type, latency_ms, status, error.unwrap_or(""), created_at],
        )?;
        Ok(())
    }
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
    })
}

#[cfg(test)]
mod tests {
    use super::{Database, Job};
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
}
