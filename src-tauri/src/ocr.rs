//! Shared OCR queue. The per-OS engine only turns image bytes into text;
//! this module owns queueing, retries, status updates, and line-joining.

use crate::capture::{CaptureError, OcrEngine};
use crate::db::{Db, OCR_DONE, OCR_FAILED, OCR_PENDING};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const RETRIES: u32 = 3;
const RETRY_PAUSE: Duration = Duration::from_millis(80);
const POLL: Duration = Duration::from_millis(200);

/// One recognized word (or line fragment) with a Vision-style bounding box:
/// origin at the lower-left, values normalized 0–1.
#[derive(Clone, Debug)]
pub struct TextBox {
    pub x: f64,
    pub y: f64,
    #[allow(dead_code)]
    pub width: f64,
    pub height: f64,
    pub text: String,
}

/// Join boxes into lines: top-to-bottom, then left-to-right within a line.
pub fn join_text_boxes(mut boxes: Vec<TextBox>) -> String {
    boxes.retain(|b| !b.text.is_empty());
    if boxes.is_empty() {
        return String::new();
    }
    let top = |b: &TextBox| b.y + b.height;
    boxes.sort_by(|a, b| {
        top(b)
            .partial_cmp(&top(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut lines: Vec<Vec<TextBox>> = Vec::new();
    for b in boxes {
        if let Some(line) = lines.last_mut() {
            let mid = b.y + b.height / 2.0;
            let ref_b = &line[0];
            let ref_mid = ref_b.y + ref_b.height / 2.0;
            let thresh = ref_b.height.max(b.height) * 0.6;
            if (mid - ref_mid).abs() <= thresh {
                line.push(b);
                continue;
            }
        }
        lines.push(vec![b]);
    }
    for line in &mut lines {
        line.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    }
    lines
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|b| b.text)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Background OCR loop. Capture enqueues a frame id and moves on; this thread
/// does the (slow) recognition and writes the result back.
pub struct OcrWorker {
    tx: Sender<i64>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl OcrWorker {
    pub fn start(db: Arc<Mutex<Db>>, data_dir: PathBuf, engine: Box<dyn OcrEngine>) -> Self {
        let (tx, rx) = mpsc::channel();
        if let Ok(guard) = db.lock() {
            if let Ok(ids) = guard.pending_ocr_ids() {
                for id in ids {
                    let _ = tx.send(id);
                }
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                match rx.recv_timeout(POLL) {
                    Ok(id) => process(id, &db, &data_dir, engine.as_ref()),
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self {
            tx,
            stop,
            handle: Some(handle),
        }
    }

    #[allow(dead_code)]
    pub fn enqueue(&self, frame_id: i64) {
        let _ = self.tx.send(frame_id);
    }

    pub fn sender(&self) -> Sender<i64> {
        self.tx.clone()
    }

    #[allow(dead_code)]
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for OcrWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn process(id: i64, db: &Mutex<Db>, data_dir: &Path, engine: &dyn OcrEngine) {
    let job = match db.lock() {
        Ok(g) => g.ocr_job(id).ok().flatten(),
        Err(_) => return,
    };
    let Some((path, status)) = job else { return };
    if status != OCR_PENDING {
        return;
    }
    let Some(rel) = path else {
        let _ = db
            .lock()
            .ok()
            .and_then(|g| g.set_ocr_result(id, "", OCR_DONE).ok());
        return;
    };
    let bytes = match std::fs::read(data_dir.join(&rel)) {
        Ok(b) => b,
        Err(_) => {
            let _ = db
                .lock()
                .ok()
                .and_then(|g| g.set_ocr_result(id, "", OCR_FAILED).ok());
            return;
        }
    };

    let mut last_err: Option<CaptureError> = None;
    for attempt in 0..RETRIES {
        match engine.recognize(&bytes) {
            Ok(text) => {
                let _ = db
                    .lock()
                    .ok()
                    .and_then(|g| g.set_ocr_result(id, &text, OCR_DONE).ok());
                return;
            }
            Err(CaptureError::Unsupported) => {
                let _ = db
                    .lock()
                    .ok()
                    .and_then(|g| g.set_ocr_result(id, "", OCR_FAILED).ok());
                return;
            }
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < RETRIES {
                    std::thread::sleep(RETRY_PAUSE * (attempt + 1));
                }
            }
        }
    }
    let _ = last_err;
    let _ = db
        .lock()
        .ok()
        .and_then(|g| g.set_ocr_result(id, "", OCR_FAILED).ok());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::OcrEngine;
    use std::sync::atomic::AtomicUsize;

    fn box_at(x: f64, y: f64, w: f64, h: f64, text: &str) -> TextBox {
        TextBox {
            x,
            y,
            width: w,
            height: h,
            text: text.into(),
        }
    }

    #[test]
    fn join_text_boxes_is_top_to_bottom_left_to_right() {
        // Vision coords: higher y is closer to the top of the image.
        let text = join_text_boxes(vec![
            box_at(0.5, 0.7, 0.2, 0.1, "right"),
            box_at(0.1, 0.7, 0.2, 0.1, "left"),
            box_at(0.1, 0.2, 0.3, 0.1, "bottom"),
        ]);
        assert_eq!(text, "left right\nbottom");
    }

    struct FakeOcr {
        flaky_attempts: AtomicUsize,
    }

    impl OcrEngine for FakeOcr {
        fn recognize(&self, image: &[u8]) -> Result<String, CaptureError> {
            if image == b"FAIL" {
                return Err(CaptureError::Other("boom".into()));
            }
            if image == b"FLAKY" {
                // First call fails, second succeeds — exercises shared retry logic.
                if self.flaky_attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                    return Err(CaptureError::Other("transient".into()));
                }
            }
            if image == b"EMPTY" {
                return Ok(String::new());
            }
            Ok(format!("read {}", String::from_utf8_lossy(image)))
        }
    }

    fn wait_idle(db: &Mutex<Db>) {
        for _ in 0..50 {
            if db.lock().unwrap().ocr_pending_count().unwrap() == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("OCR worker did not drain the queue");
    }

    #[test]
    fn worker_dedup_status_retries_and_does_not_block_on_failure() {
        let data_dir = std::env::temp_dir().join(format!(
            "locallens-ocr-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(data_dir.join("frames/1")).unwrap();
        std::fs::write(data_dir.join("frames/1/ok.bin"), b"alpha").unwrap();
        std::fs::write(data_dir.join("frames/1/fail.bin"), b"FAIL").unwrap();
        std::fs::write(data_dir.join("frames/1/later.bin"), b"beta").unwrap();
        std::fs::write(data_dir.join("frames/1/flaky.bin"), b"FLAKY").unwrap();
        std::fs::write(data_dir.join("frames/1/empty.bin"), b"EMPTY").unwrap();

        let db = Arc::new(Mutex::new(Db::open_in_memory().unwrap()));
        let sid = db.lock().unwrap().start_session(None).unwrap().id;
        let ok = db
            .lock()
            .unwrap()
            .add_frame(sid, 1, Some("frames/1/ok.bin"), "")
            .unwrap();
        let fail = db
            .lock()
            .unwrap()
            .add_frame(sid, 2, Some("frames/1/fail.bin"), "")
            .unwrap();
        let later = db
            .lock()
            .unwrap()
            .add_frame(sid, 3, Some("frames/1/later.bin"), "")
            .unwrap();
        let flaky = db
            .lock()
            .unwrap()
            .add_frame(sid, 4, Some("frames/1/flaky.bin"), "")
            .unwrap();
        let empty = db
            .lock()
            .unwrap()
            .add_frame(sid, 5, Some("frames/1/empty.bin"), "")
            .unwrap();
        // No image: migration/worker should mark done, not leave it pending forever.
        let pruned = db.lock().unwrap().add_frame(sid, 6, None, "").unwrap();

        let worker = OcrWorker::start(
            db.clone(),
            data_dir.clone(),
            Box::new(FakeOcr {
                flaky_attempts: AtomicUsize::new(0),
            }),
        );
        wait_idle(&db);
        worker.shutdown();

        let rows = db.lock().unwrap().list_frames(sid).unwrap();
        let get = |id| rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(get(ok).ocr_status, OCR_DONE);
        assert_eq!(get(ok).ocr_text, "read alpha");
        assert_eq!(get(fail).ocr_status, OCR_FAILED);
        assert_eq!(get(fail).ocr_text, "");
        assert_eq!(get(later).ocr_status, OCR_DONE);
        assert_eq!(get(later).ocr_text, "read beta");
        assert_eq!(get(flaky).ocr_status, OCR_DONE);
        assert_eq!(get(flaky).ocr_text, "read FLAKY");
        assert_eq!(get(empty).ocr_status, OCR_DONE);
        assert_eq!(get(empty).ocr_text, "");
        assert_eq!(get(pruned).ocr_status, OCR_DONE);

        let hits = db.lock().unwrap().search("alpha", None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].frame_id, hits[0].session_id, hits[0].ts),
            (ok, sid, 1)
        );
        assert!(db
            .lock()
            .unwrap()
            .search("beta", None)
            .unwrap()
            .iter()
            .any(|h| h.frame_id == later));

        let _ = std::fs::remove_dir_all(data_dir);
    }
}
