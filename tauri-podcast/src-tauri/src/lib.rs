mod commands;
mod python;

use tauri::Emitter;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(commands::transcribe::JobRegistry::default())
        .setup(|app| {
            // Emit app:ready so the frontend can trigger the setup check
            app.emit("app:ready", ()).ok();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::setup::check_setup,
            commands::setup::install_deps,
            commands::transcribe::transcribe,
            commands::transcribe::cancel_transcribe,
            commands::transcribe::list_active_jobs,
            commands::transcribe::poll_job_result,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::get_cache_dir,
            commands::settings::list_gemini_models,
            commands::files::pick_audio_file,
            commands::files::export_file,
            commands::files::open_cache_folder,
            commands::files::read_cache_file,
            commands::files::write_json_file,
            commands::files::clear_cache,
            commands::memo::generate_memo,
            commands::history::list_transcripts,
            commands::history::load_transcript,
            commands::history::delete_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
