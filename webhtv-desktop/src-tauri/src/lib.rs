mod bridge;
mod config;
mod config_commands;
mod database;
mod image_proxy;
mod library_commands;
mod live;
mod market;
mod player;
mod spider;
mod spider_commands;
mod spider_java;
mod spider_quickjs;
mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let state = state::CoreState::open(&data_dir.join("webhtv.db"))
                .map_err(std::io::Error::other)?;
            app.manage(std::sync::Arc::new(state));
            image_proxy::start(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bridge::bootstrap,
            bridge::bridge_invoke,
            bridge::bridge_console,
            bridge::bridge_network,
            bridge::bridge_inline_result,
            image_proxy::image_proxy_port,
            config_commands::config_list,
            config_commands::config_active,
            config_commands::config_load_url,
            config_commands::config_import_json,
            config_commands::config_import_file,
            config_commands::config_activate,
            config_commands::config_select_home,
            config_commands::config_delete,
            spider_commands::spider_invoke,
            library_commands::library_list_keeps,
            library_commands::library_list_history,
            library_commands::library_is_kept,
            library_commands::library_add_keep,
            library_commands::library_remove_keep,
            library_commands::library_add_history,
            library_commands::library_remove_history,
            library_commands::library_clear_history,
            live::live_sources,
            live::live_load,
            market::market_catalog,
            market::market_install,
            market::app_restart,
            player::player_open,
            player::player_surface_attach,
            player::player_surface_update,
            player::player_surface_detach,
            player::player_control,
            player::player_resolve,
            player::player_progress_get,
            player::player_progress_set,
player::player_status,
            player::player_close,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Clean up child processes (Java sidecar JVMs, mpv) so app exit doesn't
                // leave orphaned processes that hold ports/locks behind.
                let state = app.state::<state::SharedState>();
                state.spiders.invalidate_all();
                state.player.close();
            }
        });
}
