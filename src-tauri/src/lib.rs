mod capture;
mod db;

use db::{Db, Session};
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState {
    db: Mutex<Db>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
fn start_session(state: State<AppState>) -> Result<Session, String> {
    // TODO(m1): kick off the platform ScreenCapturer + OcrEngine loop here.
    state.db.lock().map_err(err)?.start_session().map_err(err)
}

#[tauri::command]
fn stop_session(state: State<AppState>) -> Result<Session, String> {
    state.db.lock().map_err(err)?
        .stop_session().map_err(err)?
        .ok_or_else(|| "no active session".to_string())
}

#[tauri::command]
fn list_sessions(state: State<AppState>) -> Result<Vec<Session>, String> {
    state.db.lock().map_err(err)?.list_sessions().map_err(err)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Everything lives in the per-user app data dir; nothing leaves the machine.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = Db::open(&dir.join("locallens.db"))?;
            app.manage(AppState { db: Mutex::new(db) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_session, stop_session, list_sessions])
        .run(tauri::generate_context!())
        .expect("error while running LocalLens");
}
