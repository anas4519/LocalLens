mod capture;
mod db;
mod ocr;

use db::{Db, FrameRow, Session};
use ocr::OcrWorker;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{Manager, State};

/// How often we try to grab a frame while a session is running.
const CAPTURE_INTERVAL: Duration = Duration::from_secs(2);

/// A running capture loop: a flag to ask it to stop, and the thread handle to wait on.
struct Recorder {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    last_error: Arc<Mutex<Option<String>>>,
}

struct AppState {
    // Arc = shared ownership across threads; Mutex = one writer at a time.
    db: Arc<Mutex<Db>>,
    data_dir: PathBuf,
    recorder: Mutex<Option<Recorder>>,
    ocr: OcrWorker,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(serde::Serialize)]
struct Status {
    has_permission: bool,
    active: Option<Session>,
    capture_error: Option<String>,
    ocr_pending: i64,
}

#[tauri::command]
fn capture_status(state: State<AppState>) -> Result<Status, String> {
    let active = state
        .db
        .lock()
        .map_err(err)?
        .active_session()
        .map_err(err)?;
    let capture_error = state
        .recorder
        .lock()
        .map_err(err)?
        .as_ref()
        .and_then(|r| r.last_error.lock().ok().and_then(|e| e.clone()));
    let ocr_pending = state
        .db
        .lock()
        .map_err(err)?
        .ocr_pending_count()
        .map_err(err)?;
    Ok(Status {
        has_permission: capture::has_permission(),
        active,
        capture_error,
        ocr_pending,
    })
}

#[tauri::command]
fn start_session(state: State<AppState>) -> Result<Session, String> {
    let mut recorder = state.recorder.lock().map_err(err)?;
    if recorder.is_some() {
        return state.db.lock().map_err(err)?.start_session().map_err(err);
    }
    // Check permission *before* creating a session, so a denied start leaves no empty row.
    let mut capturer = capture::platform_capturer();
    capturer.start().map_err(err)?;
    let session = state.db.lock().map_err(err)?.start_session().map_err(err)?;

    let frames_dir = state.data_dir.join("frames").join(session.id.to_string());
    std::fs::create_dir_all(&frames_dir).map_err(err)?;

    let stop = Arc::new(AtomicBool::new(false));
    let last_error = Arc::new(Mutex::new(None));
    let ocr_tx = state.ocr.sender();
    let (db, stop2, last_error2, sid) = (
        state.db.clone(),
        stop.clone(),
        last_error.clone(),
        session.id,
    );

    // `move` hands ownership of the captured variables to the new thread.
    let handle = std::thread::spawn(move || {
        while !stop2.load(Ordering::Relaxed) {
            let tick = Instant::now();
            match capturer.next_frame() {
                Ok(Some(frame)) => {
                    let name = format!("{}.jpg", frame.ts);
                    let rel = format!("frames/{sid}/{name}");
                    let res = std::fs::write(frames_dir.join(&name), &frame.image)
                        .map_err(err)
                        .and_then(|_| {
                            let id = db
                                .lock()
                                .map_err(err)?
                                .add_frame(sid, frame.ts, Some(&rel), "")
                                .map_err(err)?;
                            let _ = ocr_tx.send(id);
                            Ok(())
                        });
                    *last_error2.lock().unwrap() = res.err();
                }
                Ok(None) => *last_error2.lock().unwrap() = None, // unchanged screen, skip
                Err(e) => *last_error2.lock().unwrap() = Some(e.to_string()),
            }
            // Sleep in small steps so Stop feels instant.
            while tick.elapsed() < CAPTURE_INTERVAL && !stop2.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        let _ = capturer.stop();
    });

    *recorder = Some(Recorder {
        stop,
        handle,
        last_error,
    });
    Ok(session)
}

#[tauri::command]
fn stop_session(state: State<AppState>) -> Result<Session, String> {
    if let Some(r) = state.recorder.lock().map_err(err)?.take() {
        r.stop.store(true, Ordering::Relaxed);
        let _ = r.handle.join(); // wait for the in-flight frame to finish writing
    }
    state
        .db
        .lock()
        .map_err(err)?
        .stop_session()
        .map_err(err)?
        .ok_or_else(|| "no active session".to_string())
}

#[tauri::command]
fn list_sessions(state: State<AppState>) -> Result<Vec<Session>, String> {
    state.db.lock().map_err(err)?.list_sessions().map_err(err)
}

/// Frames of one session. `image_path` is made absolute so the UI can load it
/// through the asset protocol (scoped to the frames dir in tauri.conf.json).
#[tauri::command]
fn list_frames(state: State<AppState>, session_id: i64) -> Result<Vec<FrameRow>, String> {
    let mut frames = state
        .db
        .lock()
        .map_err(err)?
        .list_frames(session_id)
        .map_err(err)?;
    for f in &mut frames {
        f.image_path = f
            .image_path
            .take()
            .map(|p| state.data_dir.join(p))
            .filter(|p| p.exists()) // pruned or deleted file -> placeholder
            .map(|p| p.to_string_lossy().into_owned());
    }
    Ok(frames)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Everything lives in the per-user app data dir; nothing leaves the machine.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = Db::open(&dir.join("locallens.db"))?;
            // A session left open by a crash/quit has no capture loop behind it; close it.
            db.stop_session()?;
            let db = Arc::new(Mutex::new(db));
            let ocr = OcrWorker::start(db.clone(), dir.clone(), capture::platform_ocr());
            app.manage(AppState {
                db,
                data_dir: dir,
                recorder: Mutex::new(None),
                ocr,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_session,
            stop_session,
            list_sessions,
            capture_status,
            list_frames
        ])
        .run(tauri::generate_context!())
        .expect("error while running LocalLens");
}
