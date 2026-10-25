//! Windows placeholder. Planned: Windows.Graphics.Capture + Windows.Media.Ocr.
//! Not started until the macOS build has been used weekly for ~a month.
use super::{CaptureError, Frame, OcrEngine, ScreenCapturer};

#[allow(dead_code)]
#[derive(Default)]
pub struct WinCapturer;

impl ScreenCapturer for WinCapturer {
    fn start(&mut self) -> Result<(), CaptureError> { Err(CaptureError::Unsupported) }
    fn stop(&mut self) -> Result<(), CaptureError> { Ok(()) }
    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError> { Ok(None) }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct WinOcr;

impl OcrEngine for WinOcr {
    fn recognize(&self, _frame: &Frame) -> Result<String, CaptureError> { Err(CaptureError::Unsupported) }
}
