//! macOS capture via ScreenCaptureKit and OCR via Apple Vision.
//! TODO(m1): implement with objc2 bindings (SCStream + VNRecognizeTextRequest).
use super::{CaptureError, Frame, OcrEngine, ScreenCapturer};

#[allow(dead_code)]
#[derive(Default)]
pub struct MacCapturer;

impl ScreenCapturer for MacCapturer {
    fn start(&mut self) -> Result<(), CaptureError> {
        // TODO: request Screen Recording permission, create SCStream at ~1 frame / few seconds.
        Err(CaptureError::Unsupported)
    }
    fn stop(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }
    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError> {
        // TODO: pull latest CMSampleBuffer, diff against previous, encode.
        Ok(None)
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct VisionOcr;

impl OcrEngine for VisionOcr {
    fn recognize(&self, _frame: &Frame) -> Result<String, CaptureError> {
        // TODO: VNRecognizeTextRequest with .accurate recognition level.
        Err(CaptureError::Unsupported)
    }
}
