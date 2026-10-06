pub mod commands;
pub mod engine;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
