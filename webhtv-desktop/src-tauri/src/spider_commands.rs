use serde_json::{Map, Value};
use tauri::State;

use crate::{config::Site, spider::SpiderCall, state::SharedState};

#[tauri::command]
pub async fn spider_invoke(
    site_key: Option<String>,
    method: String,
    args: Option<Value>,
    state: State<'_, SharedState>,
) -> Result<Value, String> {
    let state = state.inner().clone();
    let call = SpiderCall::parse(&method, args.unwrap_or_else(|| Value::Object(Map::new())))?;
    let active = state
        .database
        .active_config()?
        .ok_or_else(|| "no active configuration is available".to_string())?;
    let site = resolve_site(
        &active.document.sites,
        active.summary.home_key.as_str(),
        site_key,
    )?;
    state.spiders.invoke(active.summary.id, site, call).await
}

fn resolve_site(sites: &[Site], home_key: &str, requested: Option<String>) -> Result<Site, String> {
    let requested = requested
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(home_key);
    sites
        .iter()
        .find(|site| site.key == requested)
        .cloned()
        .ok_or_else(|| format!("site `{requested}` is not in the active configuration"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sites() -> Vec<Site> {
        serde_json::from_value(json!([
            {"key":"first","name":"First","type":3,"api":"https://example.com/first.js"},
            {"key":"second","name":"Second","type":3,"api":"https://example.com/second.js"}
        ]))
        .unwrap()
    }

    #[test]
    fn site_resolution_uses_home_or_explicit_key() {
        assert_eq!(resolve_site(&sites(), "first", None).unwrap().key, "first");
        assert_eq!(
            resolve_site(&sites(), "first", Some("second".to_string()))
                .unwrap()
                .key,
            "second"
        );
        assert!(resolve_site(&sites(), "first", Some("missing".to_string())).is_err());
    }
}
