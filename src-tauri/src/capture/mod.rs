//! Platform capture + OCR live behind these traits so the core (sessions,
//! storage, search, Q&A) is shared and only a thin layer is per-OS.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// A single captured screen frame.
#[allow(dead_code)]
pub struct Frame {
    /// Unix epoch milliseconds.
    pub ts: i64,
    /// Encoded image bytes (e.g. PNG/HEIC).
    pub image: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("capture not implemented on this platform yet")]
    Unsupported,
    #[error("permission denied: {0}")]
    Permission(String),
    #[error("{0}")]
    Other(String),
}

/// Grabs frames from the screen. Implementations should skip near-duplicate
/// frames (cheap perceptual diff) before handing them to the core.
#[allow(dead_code)]
pub trait ScreenCapturer: Send + Sync {
    fn start(&mut self) -> Result<(), CaptureError>;
    fn stop(&mut self) -> Result<(), CaptureError>;
    /// Returns the next changed frame, or None if nothing changed.
    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError>;
}

/// Turns a frame into text, fully on-device.
#[allow(dead_code)]
pub trait OcrEngine: Send + Sync {
    fn recognize(&self, frame: &Frame) -> Result<String, CaptureError>;
}
