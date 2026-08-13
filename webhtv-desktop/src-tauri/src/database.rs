use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::Serialize;

use crate::config::{Site, VodConfigDocument};

const SCHEMA_VERSION: i64 = 2;

pub struct Database {
    connection: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSummary {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub desc: String,
    pub site_count: usize,
    pub active: bool,
    pub home_key: String,
    pub parse_name: String,
    pub logo: String,
    pub notice: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDetail {
    pub summary: ConfigSummary,
    pub document: VodConfigDocument,
    pub home_site: Option<Site>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path)
            .map_err(|error| format!("unable to open database {}: {error}", path.display()))?;
        Self::from_connection(connection, true)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let connection = Connection::open_in_memory()
            .map_err(|error| format!("unable to open in-memory database: {error}"))?;
        Self::from_connection(connection, false)
    }

    fn from_connection(connection: Connection, persistent: bool) -> Result<Self, String> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(database_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(database_error)?;
        if persistent {
            connection
                .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
                .map_err(database_error)?;
        }
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn save_config(
        &self,
        source: &str,
        name: &str,
        document: &VodConfigDocument,
        activate: bool,
    ) -> Result<ConfigDetail, String> {
        let source = source.trim();
        if source.is_empty() {
            return Err("configuration source cannot be empty".to_string());
        }
        let payload = serde_json::to_string(document)
            .map_err(|error| format!("unable to serialize configuration: {error}"))?;
        let home_key = document
            .home_site()
            .map(|site| site.key.clone())
            .unwrap_or_default();
        let display_name = if name.trim().is_empty() {
            source
        } else {
            name.trim()
        };
        let now = now_millis();

        let id = {
            let mut connection = self
                .connection
                .lock()
                .map_err(|_| "database is unavailable".to_string())?;
            let transaction = connection.transaction().map_err(database_error)?;
            if activate {
                transaction
                    .execute("UPDATE configs SET active = 0 WHERE config_type = 0", [])
                    .map_err(database_error)?;
            }
            transaction
                .execute(
                    "INSERT INTO configs (
                        config_type, source, name, payload, home_key, parse_name,
                        logo, notice, danmaku, site_count, active, updated_at
                     ) VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                     ON CONFLICT(source, config_type) DO UPDATE SET
                        name = excluded.name,
                        payload = excluded.payload,
                        home_key = excluded.home_key,
                        parse_name = excluded.parse_name,
                        logo = excluded.logo,
                        notice = excluded.notice,
                        danmaku = excluded.danmaku,
                        site_count = excluded.site_count,
                        active = excluded.active,
                        updated_at = excluded.updated_at",
                    params![
                        source,
                        display_name,
                        payload,
                        home_key,
                        document.parse,
                        document.logo,
                        document.notice,
                        document.danmaku,
                        i64::try_from(document.sites.len()).unwrap_or(i64::MAX),
                        i64::from(activate),
                        now,
                    ],
                )
                .map_err(database_error)?;
            let id = transaction
                .query_row(
                    "SELECT id FROM configs WHERE source = ?1 AND config_type = 0",
                    [source],
                    |row| row.get(0),
                )
                .map_err(database_error)?;
            transaction.commit().map_err(database_error)?;
            id
        };

        self.config_by_id(id)?
            .ok_or_else(|| "saved configuration could not be read".to_string())
    }

    pub fn list_configs(&self) -> Result<Vec<ConfigSummary>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT id, source, name, site_count, active, home_key, parse_name,
                        logo, notice, updated_at
                 FROM configs WHERE config_type = 0
                 ORDER BY active DESC, updated_at DESC, id DESC",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map([], summary_from_row)
            .map_err(database_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
    }

    pub fn active_config(&self) -> Result<Option<ConfigDetail>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        read_config(
            &connection,
            "SELECT id, source, name, site_count, active, home_key, parse_name,
                    logo, notice, updated_at, payload
             FROM configs WHERE config_type = 0 AND active = 1
             ORDER BY updated_at DESC LIMIT 1",
            [],
        )
    }

    pub fn config_by_id(&self, id: i64) -> Result<Option<ConfigDetail>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        read_config(
            &connection,
            "SELECT id, source, name, site_count, active, home_key, parse_name,
                    logo, notice, updated_at, payload
             FROM configs WHERE id = ?1 AND config_type = 0",
            [id],
        )
    }

    pub fn activate_config(&self, id: i64) -> Result<ConfigDetail, String> {
        {
            let mut connection = self
                .connection
                .lock()
                .map_err(|_| "database is unavailable".to_string())?;
            let transaction = connection.transaction().map_err(database_error)?;
            ensure_config_exists(&transaction, id)?;
            transaction
                .execute("UPDATE configs SET active = 0 WHERE config_type = 0", [])
                .map_err(database_error)?;
            transaction
                .execute(
                    "UPDATE configs SET active = 1, updated_at = ?1 WHERE id = ?2",
                    params![now_millis(), id],
                )
                .map_err(database_error)?;
            transaction.commit().map_err(database_error)?;
        }
        self.config_by_id(id)?
            .ok_or_else(|| "activated configuration could not be read".to_string())
    }

    pub fn select_home(&self, id: i64, site_key: &str) -> Result<ConfigDetail, String> {
        let site_key = site_key.trim();
        if site_key.is_empty() {
            return Err("site key cannot be empty".to_string());
        }
        {
            let mut connection = self
                .connection
                .lock()
                .map_err(|_| "database is unavailable".to_string())?;
            let transaction = connection.transaction().map_err(database_error)?;
            let payload: String = transaction
                .query_row("SELECT payload FROM configs WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(database_error)?
                .ok_or_else(|| format!("configuration {id} was not found"))?;
            let mut document: VodConfigDocument = serde_json::from_str(&payload)
                .map_err(|error| format!("stored configuration is invalid: {error}"))?;
            if !document.sites.iter().any(|site| site.key == site_key) {
                return Err(format!("site `{site_key}` is not in configuration {id}"));
            }
            document.home = site_key.to_string();
            let payload = serde_json::to_string(&document)
                .map_err(|error| format!("unable to serialize configuration: {error}"))?;
            transaction
                .execute(
                    "UPDATE configs SET home_key = ?1, payload = ?2, updated_at = ?3 WHERE id = ?4",
                    params![site_key, payload, now_millis(), id],
                )
                .map_err(database_error)?;
            transaction.commit().map_err(database_error)?;
        }
        self.config_by_id(id)?
            .ok_or_else(|| "updated configuration could not be read".to_string())
    }

    pub fn delete_config(&self, id: i64) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        let changed = connection
            .execute("DELETE FROM configs WHERE id = ?1", [id])
            .map_err(database_error)?;
        if changed == 0 {
            return Err(format!("configuration {id} was not found"));
        }
        Ok(())
    }

    pub fn cache_get(&self, key: &str) -> Result<String, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .query_row("SELECT value FROM cache WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map(|value| value.unwrap_or_default())
            .map_err(database_error)
    }

    pub fn cache_set(&self, key: &str, value: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .execute(
                "INSERT INTO cache (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![key, value, now_millis()],
            )
            .map_err(database_error)?;
        Ok(())
    }

    pub fn cache_delete(&self, key: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .execute("DELETE FROM cache WHERE key = ?1", [key])
            .map_err(database_error)?;
        Ok(())
    }

    pub fn list_keeps(&self) -> Result<Vec<LibraryItem>, String> {
        self.list_library("keeps")
    }

    pub fn list_history(&self) -> Result<Vec<LibraryItem>, String> {
        self.list_library("history")
    }

    pub fn is_kept(&self, site_key: &str, vod_id: &str) -> Result<bool, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .query_row(
                "SELECT 1 FROM keeps WHERE site_key = ?1 AND vod_id = ?2",
                params![site_key, vod_id],
                |_| Ok(()),
            )
            .optional()
            .map(|row| row.is_some())
            .map_err(database_error)
    }

    pub fn upsert_keep(&self, item: &LibraryInput) -> Result<LibraryItem, String> {
        self.upsert_library("keeps", item)
    }

    pub fn remove_keep(&self, site_key: &str, vod_id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .execute(
                "DELETE FROM keeps WHERE site_key = ?1 AND vod_id = ?2",
                params![site_key, vod_id],
            )
            .map_err(database_error)?;
        Ok(())
    }

    pub fn upsert_history(&self, item: &LibraryInput) -> Result<LibraryItem, String> {
        self.upsert_library("history", item)
    }

    pub fn remove_history(&self, site_key: &str, vod_id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .execute(
                "DELETE FROM history WHERE site_key = ?1 AND vod_id = ?2",
                params![site_key, vod_id],
            )
            .map_err(database_error)?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .execute("DELETE FROM history", [])
            .map_err(database_error)?;
        Ok(())
    }

    fn list_library(&self, table: &str) -> Result<Vec<LibraryItem>, String> {
        let sql = format!(
            "SELECT id, site_key, site_name, vod_id, vod_name, vod_pic, vod_remarks, updated_at
             FROM {table}
             ORDER BY updated_at DESC, id DESC
             LIMIT 500"
        );
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        let mut statement = connection.prepare(&sql).map_err(database_error)?;
        let rows = statement
            .query_map([], library_from_row)
            .map_err(database_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(database_error)
    }

    fn upsert_library(&self, table: &str, item: &LibraryInput) -> Result<LibraryItem, String> {
        let site_key = item.site_key.trim();
        let vod_id = item.vod_id.trim();
        if site_key.is_empty() || vod_id.is_empty() {
            return Err("library item requires site_key and vod_id".to_string());
        }
        let now = now_millis();
        let sql = format!(
            "INSERT INTO {table} (
                site_key, site_name, vod_id, vod_name, vod_pic, vod_remarks, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(site_key, vod_id) DO UPDATE SET
                site_name = excluded.site_name,
                vod_name = excluded.vod_name,
                vod_pic = excluded.vod_pic,
                vod_remarks = excluded.vod_remarks,
                updated_at = excluded.updated_at"
        );
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database is unavailable".to_string())?;
        connection
            .execute(
                &sql,
                params![
                    site_key,
                    item.site_name.trim(),
                    vod_id,
                    item.vod_name.trim(),
                    item.vod_pic.trim(),
                    item.vod_remarks.trim(),
                    now,
                ],
            )
            .map_err(database_error)?;
        let select = format!(
            "SELECT id, site_key, site_name, vod_id, vod_name, vod_pic, vod_remarks, updated_at
             FROM {table}
             WHERE site_key = ?1 AND vod_id = ?2"
        );
        connection
            .query_row(&select, params![site_key, vod_id], library_from_row)
            .map_err(database_error)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: i64,
    pub site_key: String,
    pub site_name: String,
    pub vod_id: String,
    pub vod_name: String,
    pub vod_pic: String,
    pub vod_remarks: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct LibraryInput {
    pub site_key: String,
    pub site_name: String,
    pub vod_id: String,
    pub vod_name: String,
    pub vod_pic: String,
    pub vod_remarks: String,
}

fn migrate(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database_error)?;
    if version > SCHEMA_VERSION {
        return Err(format!(
            "database schema {version} is newer than supported schema {SCHEMA_VERSION}"
        ));
    }
    if version == 0 {
        connection
            .execute_batch(
                "BEGIN;
                 CREATE TABLE configs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    config_type INTEGER NOT NULL DEFAULT 0,
                    source TEXT NOT NULL,
                    name TEXT NOT NULL DEFAULT '',
                    payload TEXT NOT NULL,
                    home_key TEXT NOT NULL DEFAULT '',
                    parse_name TEXT NOT NULL DEFAULT '',
                    logo TEXT NOT NULL DEFAULT '',
                    notice TEXT NOT NULL DEFAULT '',
                    danmaku TEXT NOT NULL DEFAULT '',
                    site_count INTEGER NOT NULL DEFAULT 0,
                    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
                    updated_at INTEGER NOT NULL,
                    UNIQUE(source, config_type)
                 );
                 CREATE INDEX configs_active_idx ON configs(config_type, active, updated_at DESC);
                 CREATE TABLE cache (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 1;
                 COMMIT;",
            )
            .map_err(database_error)?;
    }
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database_error)?;
    if version < 2 {
        connection
            .execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS keeps (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    site_key TEXT NOT NULL,
                    site_name TEXT NOT NULL DEFAULT '',
                    vod_id TEXT NOT NULL,
                    vod_name TEXT NOT NULL DEFAULT '',
                    vod_pic TEXT NOT NULL DEFAULT '',
                    vod_remarks TEXT NOT NULL DEFAULT '',
                    updated_at INTEGER NOT NULL,
                    UNIQUE(site_key, vod_id)
                 );
                 CREATE INDEX IF NOT EXISTS keeps_updated_idx ON keeps(updated_at DESC);
                 CREATE TABLE IF NOT EXISTS history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    site_key TEXT NOT NULL,
                    site_name TEXT NOT NULL DEFAULT '',
                    vod_id TEXT NOT NULL,
                    vod_name TEXT NOT NULL DEFAULT '',
                    vod_pic TEXT NOT NULL DEFAULT '',
                    vod_remarks TEXT NOT NULL DEFAULT '',
                    updated_at INTEGER NOT NULL,
                    UNIQUE(site_key, vod_id)
                 );
                 CREATE INDEX IF NOT EXISTS history_updated_idx ON history(updated_at DESC);
                 PRAGMA user_version = 2;
                 COMMIT;",
            )
            .map_err(database_error)?;
    }
    Ok(())
}

fn library_from_row(row: &Row<'_>) -> rusqlite::Result<LibraryItem> {
    Ok(LibraryItem {
        id: row.get(0)?,
        site_key: row.get(1)?,
        site_name: row.get(2)?,
        vod_id: row.get(3)?,
        vod_name: row.get(4)?,
        vod_pic: row.get(5)?,
        vod_remarks: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<ConfigSummary> {
    let url: String = row.get(1)?;
    let name: String = row.get(2)?;
    let desc = if name.is_empty() {
        url.clone()
    } else {
        name.clone()
    };
    let site_count: i64 = row.get(3)?;
    Ok(ConfigSummary {
        id: row.get(0)?,
        url,
        name,
        desc,
        site_count: usize::try_from(site_count.max(0)).unwrap_or(usize::MAX),
        active: row.get::<_, i64>(4)? == 1,
        home_key: row.get(5)?,
        parse_name: row.get(6)?,
        logo: row.get(7)?,
        notice: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn read_config<P>(
    connection: &Connection,
    sql: &str,
    params: P,
) -> Result<Option<ConfigDetail>, String>
where
    P: rusqlite::Params,
{
    connection
        .query_row(sql, params, |row| {
            let summary = summary_from_row(row)?;
            let payload: String = row.get(10)?;
            Ok((summary, payload))
        })
        .optional()
        .map_err(database_error)?
        .map(|(summary, payload)| {
            let mut document: VodConfigDocument = serde_json::from_str(&payload)
                .map_err(|error| format!("stored configuration is invalid: {error}"))?;
            if !summary.home_key.is_empty() {
                document.home = summary.home_key.clone();
            }
            let home_site = document.home_site().cloned();
            Ok(ConfigDetail {
                summary,
                document,
                home_site,
            })
        })
        .transpose()
}

fn ensure_config_exists(transaction: &Transaction<'_>, id: i64) -> Result<(), String> {
    let exists = transaction
        .query_row("SELECT 1 FROM configs WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map_err(database_error)?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(format!("configuration {id} was not found"))
    }
}

fn now_millis() -> i64 {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn database_error(error: rusqlite::Error) -> String {
    format!("database operation failed: {error}")
}

#[cfg(test)]
mod tests {
    use reqwest::Url;

    use super::*;
    use crate::config::{parse_config_payload, ConfigPayload};

    fn fixture() -> VodConfigDocument {
        let source = Url::parse("https://example.com/config/config.json").unwrap();
        let ConfigPayload::Document(document) = parse_config_payload(
            include_str!("../tests/fixtures/vod-config.json"),
            Some(&source),
        )
        .unwrap() else {
            panic!("expected document");
        };
        *document
    }

    #[test]
    fn config_lifecycle_persists_active_and_home_state() {
        let database = Database::open_in_memory().unwrap();
        let document = fixture();
        let saved = database
            .save_config(
                "https://example.com/config.json",
                "Example",
                &document,
                true,
            )
            .unwrap();

        assert!(saved.summary.active);
        assert_eq!(database.list_configs().unwrap().len(), 1);
        assert_eq!(
            database.active_config().unwrap().unwrap().summary.id,
            saved.summary.id
        );

        let changed = database.select_home(saved.summary.id, "alias").unwrap();
        assert_eq!(changed.summary.home_key, "alias");
        assert_eq!(changed.home_site.unwrap().key, "alias");

        database.delete_config(saved.summary.id).unwrap();
        assert!(database.active_config().unwrap().is_none());
    }

    #[test]
    fn file_database_restores_active_config_after_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "webhtv-desktop-db-{}-{}",
            std::process::id(),
            now_millis()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("webhtv.db");

        {
            let database = Database::open(&path).unwrap();
            let saved = database
                .save_config("inline://restart-test", "Restart", &fixture(), true)
                .unwrap();
            database.select_home(saved.summary.id, "alias").unwrap();
        }

        {
            let database = Database::open(&path).unwrap();
            let active = database.active_config().unwrap().unwrap();
            assert_eq!(active.summary.name, "Restart");
            assert_eq!(active.summary.home_key, "alias");
            assert_eq!(active.home_site.unwrap().key, "alias");
        }

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cache_round_trip_is_persistent() {
        let database = Database::open_in_memory().unwrap();
        database.cache_set("cache_demo_theme", "dark").unwrap();
        assert_eq!(database.cache_get("cache_demo_theme").unwrap(), "dark");
        database.cache_delete("cache_demo_theme").unwrap();
        assert_eq!(database.cache_get("cache_demo_theme").unwrap(), "");
    }

    #[test]
    fn keep_and_history_round_trip() {
        let database = Database::open_in_memory().unwrap();
        let item = LibraryInput {
            site_key: "db".into(),
            site_name: "Douban".into(),
            vod_id: "msearch:1".into(),
            vod_name: "Demo".into(),
            vod_pic: "https://example.com/a.jpg".into(),
            vod_remarks: "score".into(),
        };
        database.upsert_keep(&item).unwrap();
        assert!(database.is_kept("db", "msearch:1").unwrap());
        assert_eq!(database.list_keeps().unwrap().len(), 1);
        database.upsert_history(&item).unwrap();
        database
            .upsert_history(&LibraryInput {
                vod_name: "Demo 2".into(),
                ..item.clone()
            })
            .unwrap();
        assert_eq!(database.list_history().unwrap()[0].vod_name, "Demo 2");
        database.remove_keep("db", "msearch:1").unwrap();
        assert!(!database.is_kept("db", "msearch:1").unwrap());
        database.clear_history().unwrap();
        assert!(database.list_history().unwrap().is_empty());
    }
}
