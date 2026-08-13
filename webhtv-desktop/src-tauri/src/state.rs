use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{database::Database, player::PlayerManager, spider::SpiderManager};

pub struct CoreState {
    pub database: Database,
    pub http: reqwest::Client,
    pub spiders: SpiderManager,
    pub player: PlayerManager,
    pub inline_results: Mutex<HashMap<String, String>>,
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

pub type SharedState = Arc<CoreState>;
