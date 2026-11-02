//! Platform capture + OCR live behind these traits so the core (sessions,
//! storage, search, Q&A) is shared and only a thin layer is per-OS.

pub mod capturer;
pub mod dedup;
pub use capturer::{Capturer, RawImage, ScreenSource};
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

/// The screen source for the current OS (the only cfg'd choice point).
#[cfg(target_os = "macos")]
pub type PlatformSource = macos::MacSource;
#[cfg(target_os = "windows")]
pub type PlatformSource = windows::WinSource;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub type PlatformSource = NoSource;

pub type PlatformCapturer = Capturer<PlatformSource>;

/// The capturer for the current OS: shared logic over the per-OS source.
pub fn platform_capturer() -> Box<dyn ScreenCapturer> {
    Box::new(PlatformCapturer::new(PlatformSource::default()))
}

/// Screen-capture permission status for the current OS.
pub fn has_permission() -> bool {
    PlatformSource::default().has_permission()
}

/// Fallback for OSes we don't target yet.
#[allow(dead_code)]
#[derive(Default)]
pub struct NoSource;
impl ScreenSource for NoSource {
    fn available(&self) -> Result<(), CaptureError> { Err(CaptureError::Unsupported) }
    fn has_permission(&self) -> bool { true }
    fn permission_hint(&self) -> &'static str { "" }
    fn grab(&mut self, _max_edge: usize) -> Result<RawImage, CaptureError> { Err(CaptureError::Unsupported) }
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
