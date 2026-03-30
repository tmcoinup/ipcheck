use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use rusqlite::{Connection, params};
use rusqlite::OptionalExtension;
use thiserror::Error;
use tracing::{error, info};

use crate::domain::models::{AppStateSnapshot, CheckResult, ProxyEntry, ProxySpec, RealIpHistoryEntry};

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("database error: {0}")]
    Database(String),
}

pub trait AppRepository: Send + Sync {
    fn init(&self) -> Result<(), RepoError>;
    fn load_snapshot(&self) -> Result<AppStateSnapshot, RepoError>;
    fn save_token(&self, token: &str) -> Result<(), RepoError>;
    fn insert_proxies(&self, proxies: &[ProxySpec]) -> Result<(), RepoError>;
    fn get_proxy_id_by_raw(&self, raw: &str) -> Result<Option<i64>, RepoError>;
    fn update_real_ip(&self, proxy_id: i64, real_ip: &str, checked_at: &str) -> Result<(), RepoError>;
    fn clear_real_ip(&self, proxy_id: i64) -> Result<(), RepoError>;
    fn clear_all_real_ips(&self) -> Result<(), RepoError>;
    fn get_real_ip_history(&self, proxy_id: i64) -> Result<Vec<RealIpHistoryEntry>, RepoError>;
    fn delete_results_for_proxy(&self, proxy_id: i64) -> Result<(), RepoError>;
    fn insert_result(&self, result: &CheckResult) -> Result<(), RepoError>;
    fn clear_proxies(&self) -> Result<(), RepoError>;
    fn delete_proxy(&self, proxy_id: i64) -> Result<(), RepoError>;
}

#[derive(Clone)]
pub struct SqliteRepository {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteRepository {
    pub fn new(db_path: PathBuf) -> anyhow::Result<Self> {
        let conn = Connection::open(&db_path)
            .or_else(|_| Connection::open("ipcheck.db"))
            .or_else(|_| Connection::open_in_memory())
            .with_context(|| "failed to open sqlite database file")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn map_err(err: impl std::fmt::Display) -> RepoError {
        RepoError::Database(err.to_string())
    }
}

impl AppRepository for SqliteRepository {
    fn init(&self) -> Result<(), RepoError> {
        info!("repo init start");
        {
            let guard = self.conn.lock().map_err(Self::map_err)?;
            guard
                .execute_batch(
                    r#"
                    CREATE TABLE IF NOT EXISTS settings (
                        key TEXT PRIMARY KEY,
                        value TEXT NOT NULL
                    );

                    CREATE TABLE IF NOT EXISTS proxies (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        raw TEXT NOT NULL UNIQUE,
                        username TEXT NOT NULL,
                        password TEXT NOT NULL,
                        host TEXT NOT NULL,
                        port INTEGER NOT NULL,
                        created_at TEXT,
                        last_real_ip TEXT,
                        updated_at TEXT
                    );

                    CREATE TABLE IF NOT EXISTS results (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        proxy_id INTEGER NOT NULL,
                        source_proxy TEXT NOT NULL,
                        real_ip TEXT NOT NULL,
                        base_json TEXT NOT NULL,
                        overall_json TEXT NOT NULL,
                        checked_at TEXT NOT NULL
                    );

                    CREATE TABLE IF NOT EXISTS real_ip_history (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        proxy_id INTEGER NOT NULL,
                        real_ip TEXT NOT NULL,
                        observed_at TEXT NOT NULL
                    );
                    "#,
                )
                .map_err(|e| {
                    error!(error = %e, "repo init failed");
                    Self::map_err(e)
                })?;
        }
        self.ensure_proxy_created_at_column()?;
        info!("repo init success");
        Ok(())
    }

    fn load_snapshot(&self) -> Result<AppStateSnapshot, RepoError> {
        info!("repo load_snapshot");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        let token_result = guard
            .query_row(
                "SELECT value FROM settings WHERE key = 'token'",
                [],
                |row| row.get::<_, String>(0),
            );
        let token = match token_result {
            Ok(value) => value,
            Err(_) => String::new(),
        };

        let mut stmt = guard
            .prepare(
                "SELECT id, raw, username, password, host, port, created_at, last_real_ip, updated_at
                 FROM proxies ORDER BY id ASC",
            )
            .map_err(Self::map_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ProxyEntry {
                    id: row.get(0)?,
                    raw: row.get(1)?,
                    username: row.get(2)?,
                    password: row.get(3)?,
                    host: row.get(4)?,
                    port: row.get::<_, u16>(5)?,
                    created_at: row.get(6)?,
                    last_real_ip: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(Self::map_err)?;

        let proxies = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(Self::map_err)?;

        let mut stmt = guard
            .prepare(
                "SELECT proxy_id, source_proxy, real_ip, base_json, overall_json, checked_at
                 FROM results
                 WHERE id IN (SELECT MAX(id) FROM results GROUP BY proxy_id)
                 ORDER BY proxy_id ASC",
            )
            .map_err(Self::map_err)?;
        let rows = stmt
            .query_map([], |row| {
                let base_json: String = row.get(3)?;
                let overall_json: String = row.get(4)?;
                let base = match serde_json::from_str(&base_json) {
                    Ok(value) => value,
                    Err(_) => crate::domain::models::BaseData::default(),
                };
                let overall = match serde_json::from_str(&overall_json) {
                    Ok(value) => value,
                    Err(_) => crate::domain::models::OverallData::default(),
                };
                Ok(CheckResult {
                    proxy_id: row.get(0)?,
                    source_proxy: row.get(1)?,
                    real_ip: row.get(2)?,
                    base,
                    overall,
                    checked_at: row.get(5)?,
                })
            })
            .map_err(Self::map_err)?;
        let results = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(Self::map_err)?;

        Ok(AppStateSnapshot {
            token,
            proxies,
            results,
        })
    }

    fn save_token(&self, token: &str) -> Result<(), RepoError> {
        info!(token_len = token.len(), "repo save_token");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        guard
            .execute(
                "INSERT INTO settings(key, value) VALUES('token', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [token],
            )
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn insert_proxies(&self, proxies: &[ProxySpec]) -> Result<(), RepoError> {
        info!(count = proxies.len(), "repo insert_proxies");
        let mut guard = self.conn.lock().map_err(Self::map_err)?;
        let tx = guard.transaction().map_err(Self::map_err)?;
        for p in proxies {
            tx.execute(
                "INSERT OR IGNORE INTO proxies(raw, username, password, host, port, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now', 'localtime'))",
                params![p.raw, p.username, p.password, p.host, p.port],
            )
            .map_err(Self::map_err)?;
        }
        tx.commit().map_err(Self::map_err)?;
        Ok(())
    }

    fn get_proxy_id_by_raw(&self, raw: &str) -> Result<Option<i64>, RepoError> {
        info!(raw = %raw, "repo get_proxy_id_by_raw");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        let mut stmt = guard
            .prepare("SELECT id FROM proxies WHERE raw = ?1 LIMIT 1")
            .map_err(Self::map_err)?;
        let mut rows = stmt.query([raw]).map_err(Self::map_err)?;
        let row_opt = rows.next().map_err(Self::map_err)?;
        if let Some(row) = row_opt {
            let id = row.get::<_, i64>(0).map_err(Self::map_err)?;
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }

    fn update_real_ip(&self, proxy_id: i64, real_ip: &str, checked_at: &str) -> Result<(), RepoError> {
        info!(proxy_id, real_ip, checked_at, "repo update_real_ip");
        let guard = self.conn.lock().map_err(Self::map_err)?;

        let real_ip_trim = real_ip.trim();
        let last_real_ip: Option<String> = guard
            .query_row(
                "SELECT real_ip FROM real_ip_history WHERE proxy_id = ?1 ORDER BY id DESC LIMIT 1",
                [proxy_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Self::map_err)?;

        let changed = match last_real_ip {
            Some(prev) => prev.trim() != real_ip_trim,
            None => true,
        };

        if changed {
            guard
                .execute(
                    "INSERT INTO real_ip_history(proxy_id, real_ip, observed_at) VALUES (?1, ?2, ?3)",
                    params![proxy_id, real_ip_trim, checked_at],
                )
                .map_err(Self::map_err)?;
        }

        guard
            .execute(
                "UPDATE proxies SET last_real_ip = ?1, updated_at = ?2 WHERE id = ?3",
                params![real_ip_trim, checked_at, proxy_id],
            )
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn clear_real_ip(&self, proxy_id: i64) -> Result<(), RepoError> {
        info!(proxy_id, "repo clear_real_ip");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        guard
            .execute(
                "UPDATE proxies SET last_real_ip = NULL, updated_at = NULL WHERE id = ?1",
                [proxy_id],
            )
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn clear_all_real_ips(&self) -> Result<(), RepoError> {
        info!("repo clear_all_real_ips");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        guard
            .execute(
                "UPDATE proxies SET last_real_ip = NULL, updated_at = NULL",
                [],
            )
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn get_real_ip_history(&self, proxy_id: i64) -> Result<Vec<RealIpHistoryEntry>, RepoError> {
        info!(proxy_id, "repo get_real_ip_history");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        let mut stmt = guard
            .prepare(
                "SELECT id, proxy_id, real_ip, observed_at
                 FROM real_ip_history
                 WHERE proxy_id = ?1
                 ORDER BY id DESC
                 LIMIT 200",
            )
            .map_err(Self::map_err)?;
        let rows = stmt
            .query_map([proxy_id], |row| {
                Ok(RealIpHistoryEntry {
                    id: row.get(0)?,
                    proxy_id: row.get(1)?,
                    real_ip: row.get(2)?,
                    observed_at: row.get(3)?,
                })
            })
            .map_err(Self::map_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Self::map_err)
    }

    fn delete_results_for_proxy(&self, proxy_id: i64) -> Result<(), RepoError> {
        info!(proxy_id, "repo delete_results_for_proxy");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        guard
            .execute("DELETE FROM results WHERE proxy_id = ?1", [proxy_id])
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn insert_result(&self, result: &CheckResult) -> Result<(), RepoError> {
        info!(
            proxy_id = result.proxy_id,
            real_ip = %result.real_ip,
            checked_at = %result.checked_at,
            "repo insert_result"
        );
        let base_json = serde_json::to_string(&result.base).map_err(Self::map_err)?;
        let overall_json = serde_json::to_string(&result.overall).map_err(Self::map_err)?;
        let guard = self.conn.lock().map_err(Self::map_err)?;
        guard
            .execute(
                "DELETE FROM results WHERE proxy_id = ?1",
                [result.proxy_id],
            )
            .map_err(Self::map_err)?;
        guard
            .execute(
                "INSERT INTO results(proxy_id, source_proxy, real_ip, base_json, overall_json, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    result.proxy_id,
                    result.source_proxy,
                    result.real_ip,
                    base_json,
                    overall_json,
                    result.checked_at
                ],
            )
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn clear_proxies(&self) -> Result<(), RepoError> {
        info!("repo clear_proxies");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        guard
            .execute("DELETE FROM results", [])
            .map_err(Self::map_err)?;
        guard
            .execute("DELETE FROM proxies", [])
            .map_err(Self::map_err)?;
        guard
            .execute("DELETE FROM real_ip_history", [])
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn delete_proxy(&self, proxy_id: i64) -> Result<(), RepoError> {
        info!(proxy_id, "repo delete_proxy");
        let guard = self.conn.lock().map_err(Self::map_err)?;
        guard
            .execute("DELETE FROM results WHERE proxy_id = ?1", [proxy_id])
            .map_err(Self::map_err)?;
        guard
            .execute("DELETE FROM proxies WHERE id = ?1", [proxy_id])
            .map_err(Self::map_err)?;
        guard
            .execute("DELETE FROM real_ip_history WHERE proxy_id = ?1", [proxy_id])
            .map_err(Self::map_err)?;
        Ok(())
    }
}

impl SqliteRepository {
    fn ensure_proxy_created_at_column(&self) -> Result<(), RepoError> {
        let guard = self.conn.lock().map_err(Self::map_err)?;
        let mut stmt = guard
            .prepare("PRAGMA table_info(proxies)")
            .map_err(Self::map_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(Self::map_err)?;
        let mut has_created_at = false;
        for row in rows {
            let col = row.map_err(Self::map_err)?;
            if col == "created_at" {
                has_created_at = true;
                break;
            }
        }
        if !has_created_at {
            guard
                .execute("ALTER TABLE proxies ADD COLUMN created_at TEXT", [])
                .map_err(Self::map_err)?;
            guard
                .execute(
                    "UPDATE proxies SET created_at = datetime('now', 'localtime') WHERE created_at IS NULL",
                    [],
                )
                .map_err(Self::map_err)?;
        }
        Ok(())
    }
}
