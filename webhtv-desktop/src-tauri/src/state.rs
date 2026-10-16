use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::{database::Database, player::PlayerManager, spider::SpiderManager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugLogEntry {
    pub timestamp: i64,
    pub level: String,
    pub message: String,
    pub module: String,
}

pub struct CoreState {
    pub database: Database,
    pub http: reqwest::Client,
    pub spiders: SpiderManager,
    pub player: PlayerManager,
    pub inline_results: Mutex<HashMap<String, String>>,
    pub debug_logs: Mutex<VecDeque<DebugLogEntry>>,
    pub started_at: Instant,
}

impl CoreState {
    pub fn open(database_path: &Path) -> Result<Self, String> {
        let spider_dir = database_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("spiders");
        Ok(Self {
            database: Database::open(database_path)?,
            http: http_client()?,
            spiders: SpiderManager::with_work_dir(spider_dir),
            player: PlayerManager::default(),
            inline_results: Mutex::new(HashMap::new()),
            debug_logs: Mutex::new(VecDeque::new()),
            started_at: Instant::now(),
        })
    }
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            database: Database::open_in_memory().expect("in-memory database must open"),
            http: http_client().expect("HTTP client must initialize"),
            spiders: SpiderManager::default(),
            player: PlayerManager::default(),
            inline_results: Mutex::new(HashMap::new()),
            debug_logs: Mutex::new(VecDeque::new()),
            started_at: Instant::now(),
        }
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("WebHomeTV-Desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("unable to initialize HTTP client: {error}"))
}

impl CoreState {
    pub fn debug_log(&self, level: &str, module: &str, message: &str) {
        let now = now_millis();
        let entry = DebugLogEntry {
            timestamp: now,
            level: level.to_string(),
            message: message.to_string(),
            module: module.to_string(),
        };
        let mut logs = self.debug_logs.lock().unwrap_or_else(|e| e.into_inner());
        logs.push_back(entry);
        // Keep only the last 1000 entries
        while logs.len() > 1000 {
            logs.pop_front();
        }
    }
}

pub type SharedState = Arc<CoreState>;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
