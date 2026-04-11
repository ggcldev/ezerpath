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
