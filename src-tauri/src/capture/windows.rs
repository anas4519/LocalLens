//! Windows placeholder. Planned: Windows.Graphics.Capture + Windows.Media.Ocr.
//! Not started until the macOS build has been used weekly for ~a month.
use super::{CaptureError, OcrEngine, RawImage, ScreenSource};

#[derive(Default)]
pub struct WinSource;

impl ScreenSource for WinSource {
    fn available(&self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
    fn has_permission(&self) -> bool {
        true
    }
    fn permission_hint(&self) -> &'static str {
        ""
    }
    fn grab(&mut self, _max_edge: usize) -> Result<RawImage, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct WinOcr;

impl OcrEngine for WinOcr {
    fn recognize(&self, _image: &[u8]) -> Result<String, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}
