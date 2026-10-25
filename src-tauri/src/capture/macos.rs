//! macOS capture via ScreenCaptureKit (`SCScreenshotManager`, macOS 14+).
//! OCR via Apple Vision comes next milestone.
use super::dedup::{encode_jpeg, Thumb};
use super::{CaptureError, Frame, OcrEngine, ScreenCapturer};
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::SCScreenshotManager;
use std::time::{SystemTime, UNIX_EPOCH};

/// Longest edge we keep. Plenty for OCR, keeps JPEGs ~150-400 KB.
const MAX_EDGE: usize = 1920;
const JPEG_QUALITY: u8 = 75;

// CoreGraphics C functions (macOS 10.15+). `extern "C"` declares functions
// that live in a system library; calling them is `unsafe` because Rust
// can't check foreign code.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// Has the user granted Screen Recording to this app (or the terminal running it)?
pub fn has_screen_permission() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Shows the system prompt the first time; afterwards macOS just returns false
/// until the user flips the switch in System Settings and restarts the app.
pub fn request_screen_permission() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

#[derive(Default)]
pub struct MacCapturer {
    last: Option<Thumb>,
    started: bool,
}

impl MacCapturer {
    fn grab(&self) -> Result<(Vec<u8>, u32, u32), CaptureError> {
        let other = |e: &dyn std::fmt::Display| CaptureError::Other(e.to_string());
        let content = SCShareableContent::get().map_err(|e| other(&e))?;
        // First display = main display.
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| CaptureError::Other("no display found".into()))?;
        let (w, h) = fit(display.width() as usize * 2, display.height() as usize * 2);
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();
        let config = SCStreamConfiguration::new().with_width(w as u32).with_height(h as u32);
        let image = SCScreenshotManager::capture_image(&filter, &config).map_err(|e| other(&e))?;
        let rgba = image.rgba_data().map_err(|e| other(&e))?;
        Ok((rgba, image.width() as u32, image.height() as u32))
    }
}

/// Scale (w, h) down so the longest edge is at most MAX_EDGE.
fn fit(w: usize, h: usize) -> (usize, usize) {
    let longest = w.max(h).max(1);
    if longest <= MAX_EDGE {
        return (w, h);
    }
    (w * MAX_EDGE / longest, h * MAX_EDGE / longest)
}

impl ScreenCapturer for MacCapturer {
    fn start(&mut self) -> Result<(), CaptureError> {
        if !has_screen_permission() {
            request_screen_permission();
            return Err(CaptureError::Permission(
                "Screen Recording is off for LocalLens. Enable it in System Settings → Privacy & Security → Screen Recording, then quit and reopen the app.".into(),
            ));
        }
        self.last = None;
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.started = false;
        self.last = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError> {
        if !self.started {
            return Ok(None);
        }
        let (rgba, width, height) = self.grab()?;
        let thumb = Thumb::from_rgba(&rgba, width, height)
            .ok_or_else(|| CaptureError::Other("unexpected frame buffer size".into()))?;
        if !thumb.is_new(self.last.as_ref()) {
            return Ok(None);
        }
        self.last = Some(thumb);
        let image = encode_jpeg(&rgba, width, height, JPEG_QUALITY).map_err(CaptureError::Other)?;
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        Ok(Some(Frame { ts, image, width, height }))
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct VisionOcr;

impl OcrEngine for VisionOcr {
    fn recognize(&self, _frame: &Frame) -> Result<String, CaptureError> {
        // TODO(m2): VNRecognizeTextRequest with .accurate recognition level.
        Err(CaptureError::Unsupported)
    }
}
