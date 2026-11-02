//! Shared capture logic. Each OS only implements the tiny `ScreenSource`
//! trait (permission + "give me pixels"); `Capturer<S>` does everything else:
//! downscaling, duplicate skipping, JPEG encoding, timestamps, start/stop state.
use super::dedup::{encode_jpeg, Thumb};
use super::{CaptureError, Frame, ScreenCapturer};
use std::time::{SystemTime, UNIX_EPOCH};

/// Longest edge we keep. Plenty for OCR, keeps JPEGs ~150-400 KB.
pub const MAX_EDGE: usize = 1920;
pub const JPEG_QUALITY: u8 = 75;

/// Raw pixels straight from the OS: tightly packed RGBA.
pub struct RawImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The per-OS part. Keep it thin: no dedup/encoding here.
pub trait ScreenSource: Send {
    /// Is capture implemented on this OS at all?
    fn available(&self) -> Result<(), CaptureError> {
        Ok(())
    }
    fn has_permission(&self) -> bool;
    /// Ask the OS for permission (shows a prompt where supported).
    fn request_permission(&self) -> bool {
        self.has_permission()
    }
    /// User-facing instructions shown when permission is missing.
    fn permission_hint(&self) -> &'static str;
    /// Grab the main display, longest edge at most `max_edge` pixels.
    fn grab(&mut self, max_edge: usize) -> Result<RawImage, CaptureError>;
}

/// Scale (w, h) down so the longest edge is at most `max_edge`.
pub fn fit(w: usize, h: usize, max_edge: usize) -> (usize, usize) {
    let longest = w.max(h).max(1);
    if longest <= max_edge {
        return (w, h);
    }
    (w * max_edge / longest, h * max_edge / longest)
}

pub struct Capturer<S: ScreenSource> {
    source: S,
    last: Option<Thumb>,
    started: bool,
}

impl<S: ScreenSource> Capturer<S> {
    pub fn new(source: S) -> Self {
        Self { source, last: None, started: false }
    }
}

impl<S: ScreenSource> ScreenCapturer for Capturer<S> {
    fn start(&mut self) -> Result<(), CaptureError> {
        self.source.available()?;
        if !self.source.has_permission() {
            self.source.request_permission();
            return Err(CaptureError::Permission(self.source.permission_hint().into()));
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
        let RawImage { rgba, width, height } = self.source.grab(MAX_EDGE)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic fake screen: returns queued brightness levels as solid frames.
    struct FakeSource {
        permitted: bool,
        frames: Vec<u8>,
    }

    impl ScreenSource for FakeSource {
        fn has_permission(&self) -> bool { self.permitted }
        fn permission_hint(&self) -> &'static str { "grant it" }
        fn grab(&mut self, max_edge: usize) -> Result<RawImage, CaptureError> {
            let v = self.frames.remove(0);
            let (w, h) = fit(640, 360, max_edge);
            let (w, h) = (w as u32, h as u32);
            Ok(RawImage { rgba: (0..w * h).flat_map(|_| [v, v, v, 255]).collect(), width: w, height: h })
        }
    }

    #[test]
    fn fit_caps_longest_edge() {
        assert_eq!(fit(1440, 900, 1920), (1440, 900));
        assert_eq!(fit(3840, 2160, 1920), (1920, 1080));
        assert_eq!(fit(1080, 3840, 1920), (540, 1920));
        assert_eq!(fit(0, 0, 1920), (0, 0));
    }

    #[test]
    fn capturer_dedups_and_encodes() {
        let mut c = Capturer::new(FakeSource { permitted: true, frames: vec![20, 21, 200] });
        assert!(c.next_frame().unwrap().is_none(), "not started yet");
        c.start().unwrap();
        let first = c.next_frame().unwrap().expect("first frame is always new");
        assert_eq!(&first.image[..2], &[0xFF, 0xD8]); // JPEG magic
        assert_eq!((first.width, first.height), (640, 360));
        assert!(c.next_frame().unwrap().is_none(), "near-identical frame skipped");
        assert!(c.next_frame().unwrap().is_some(), "changed frame kept");
    }

    #[test]
    fn capturer_reports_missing_permission() {
        let mut c = Capturer::new(FakeSource { permitted: false, frames: vec![] });
        let e = c.start().unwrap_err();
        assert_eq!(e.to_string(), "permission denied: grant it");
    }
}
