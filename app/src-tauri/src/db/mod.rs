use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Option<i64>,
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub company: String,
    pub pay: String,
    pub posted_at: String,
    pub url: String,
    pub summary: String,
    pub keyword: String,
    pub scraped_at: String,
    pub is_new: bool,
    pub watchlisted: bool,
}

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
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

            INSERT OR IGNORE INTO keywords (keyword) VALUES ('seo specialist');
            INSERT OR IGNORE INTO keywords (keyword) VALUES ('link building');
            INSERT OR IGNORE INTO keywords (keyword) VALUES ('outreach');
            INSERT OR IGNORE INTO keywords (keyword) VALUES ('content writer');
            "
        )?;

        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn insert_job(&self, job: &Job) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let result = conn.execute(
            "INSERT OR IGNORE INTO jobs (source, source_id, title, company, pay, posted_at, url, summary, keyword, scraped_at, is_new)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                job.source, job.source_id, job.title, job.company, job.pay,
                job.posted_at, job.url, job.summary, job.keyword, job.scraped_at,
                job.is_new as i32,
            ],
        )?;
        Ok(result > 0) // true if inserted, false if duplicate
    }

    pub fn get_jobs(&self, keyword: Option<&str>, watchlisted_only: bool) -> Result<Vec<Job>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut query = String::from(
            "SELECT id, source, source_id, title, company, pay, posted_at, url, summary, keyword, scraped_at, is_new, watchlisted
             FROM jobs WHERE 1=1"
        );

        if watchlisted_only {
            query.push_str(" AND watchlisted = 1");
        }
        if keyword.is_some() {
            query.push_str(" AND keyword = ?1");
        }
        query.push_str(" ORDER BY posted_at DESC");

        let mut stmt = conn.prepare(&query)?;

        let rows = if let Some(kw) = keyword {
            stmt.query_map(params![kw], row_to_job)?
        } else {
            stmt.query_map([], row_to_job)?
        };

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    pub fn toggle_watchlist(&self, job_id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT keyword FROM keywords ORDER BY keyword")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut keywords = Vec::new();
        for row in rows {
            keywords.push(row?);
        }
        Ok(keywords)
    }

    pub fn add_keyword(&self, keyword: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT OR IGNORE INTO keywords (keyword) VALUES (?1)", params![keyword])?;
        Ok(())
    }

    pub fn remove_keyword(&self, keyword: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
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
        pay: row.get(5)?,
        posted_at: row.get(6)?,
        url: row.get(7)?,
        summary: row.get(8)?,
        keyword: row.get(9)?,
        scraped_at: row.get(10)?,
        is_new: row.get::<_, i32>(11)? != 0,
        watchlisted: row.get::<_, i32>(12)? != 0,
    })
}
