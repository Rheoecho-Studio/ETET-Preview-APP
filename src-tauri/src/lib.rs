pub mod commands;
pub mod engine;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::pick_model,
            commands::load_model,
            commands::unload_model,
            commands::generate,
            commands::stop_generation,
            commands::read_file_text,
            commands::read_file_base64,
            commands::web_search,
            commands::system_info,
        ]);

    // iOS + Android only: enable edge-to-edge (transparent system bars) so the
    // status bar / home indicator no longer overlap the web content and the
    // CSS safe-area insets in app.css drive the layout. No-op on desktop.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        builder = builder.plugin(tauri_plugin_edge_to_edge::init());
    }

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}