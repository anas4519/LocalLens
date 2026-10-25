//! Platform capture + OCR live behind these traits so the core (sessions,
//! storage, search, Q&A) is shared and only a thin layer is per-OS.

pub mod dedup;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// A single captured screen frame.
#[allow(dead_code)]
pub struct Frame {
    /// Unix epoch milliseconds.
    pub ts: i64,
    /// Encoded JPEG bytes.
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

/// The capturer for the current OS.
pub fn platform_capturer() -> Box<dyn ScreenCapturer> {
    #[cfg(target_os = "macos")]
    return Box::new(macos::MacCapturer::default());
    #[cfg(target_os = "windows")]
    return Box::new(windows::WinCapturer::default());
    #[allow(unreachable_code)]
    Box::new(NoCapturer)
}

/// Screen-capture permission status for the current OS.
pub fn has_permission() -> bool {
    #[cfg(target_os = "macos")]
    return macos::has_screen_permission();
    #[allow(unreachable_code)]
    true
}

struct NoCapturer;
impl ScreenCapturer for NoCapturer {
    fn start(&mut self) -> Result<(), CaptureError> { Err(CaptureError::Unsupported) }
    fn stop(&mut self) -> Result<(), CaptureError> { Ok(()) }
    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError> { Ok(None) }
}

/// Grabs frames from the screen. Implementations should skip near-duplicate
/// frames (cheap perceptual diff) before handing them to the core.
#[allow(dead_code)]
pub trait ScreenCapturer: Send {
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
