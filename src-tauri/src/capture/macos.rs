//! macOS screen source via ScreenCaptureKit (`SCScreenshotManager`, macOS 14+).
//! Only the OS-specific bits live here; see `capturer.rs` for the shared logic.
//! OCR via Apple Vision comes next milestone.
use super::capturer::fit;
use super::{CaptureError, Frame, OcrEngine, RawImage, ScreenSource};
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::SCScreenshotManager;

// CoreGraphics C functions (macOS 10.15+). `extern "C"` declares functions
// that live in a system library; calling them is `unsafe` because Rust
// can't check foreign code.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[derive(Default)]
pub struct MacSource;

impl ScreenSource for MacSource {
    /// Has the user granted Screen Recording to this app (or the terminal running it)?
    fn has_permission(&self) -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    /// Shows the system prompt the first time; afterwards macOS just returns false
    /// until the user flips the switch in System Settings and restarts the app.
    fn request_permission(&self) -> bool {
        unsafe { CGRequestScreenCaptureAccess() }
    }

    fn permission_hint(&self) -> &'static str {
        "Screen Recording is off for LocalLens. Enable it in System Settings → Privacy & Security → Screen Recording, then quit and reopen the app."
    }

    fn grab(&mut self, max_edge: usize) -> Result<RawImage, CaptureError> {
        let other = |e: &dyn std::fmt::Display| CaptureError::Other(e.to_string());
        let content = SCShareableContent::get().map_err(|e| other(&e))?;
        // First display = main display.
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| CaptureError::Other("no display found".into()))?;
        let (w, h) = fit(display.width() as usize * 2, display.height() as usize * 2, max_edge);
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();
        let config = SCStreamConfiguration::new().with_width(w as u32).with_height(h as u32);
        let image = SCScreenshotManager::capture_image(&filter, &config).map_err(|e| other(&e))?;
        let rgba = image.rgba_data().map_err(|e| other(&e))?;
        Ok(RawImage { rgba, width: image.width() as u32, height: image.height() as u32 })
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
