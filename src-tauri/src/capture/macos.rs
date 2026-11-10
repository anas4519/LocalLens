//! macOS screen source via ScreenCaptureKit (`SCScreenshotManager`, macOS 14+).
//! Only the OS-specific bits live here; see `capturer.rs` for the shared logic.
//! OCR: Apple Vision (`VNRecognizeTextRequest`), fully on-device.
use super::capturer::fit;
use super::{CaptureError, OcrEngine, RawImage, ScreenSource};
use crate::ocr::{join_text_boxes, TextBox};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use objc2_vision::{
    VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequestTextRecognitionLevel,
};
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
        let (w, h) = fit(
            display.width() as usize * 2,
            display.height() as usize * 2,
            max_edge,
        );
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();
        let config = SCStreamConfiguration::new()
            .with_width(w as u32)
            .with_height(h as u32);
        let image = SCScreenshotManager::capture_image(&filter, &config).map_err(|e| other(&e))?;
        let rgba = image.rgba_data().map_err(|e| other(&e))?;
        Ok(RawImage {
            rgba,
            width: image.width() as u32,
            height: image.height() as u32,
        })
    }
}

#[derive(Default)]
pub struct VisionOcr;

impl OcrEngine for VisionOcr {
    fn recognize(&self, image: &[u8]) -> Result<String, CaptureError> {
        // Drain temporary Objective-C objects after each call so the worker
        // thread doesn't leak autoreleased memory.
        objc2::rc::autoreleasepool(|_| recognize_inner(image))
    }
}

fn recognize_inner(image: &[u8]) -> Result<String, CaptureError> {
    let other = |e: &dyn std::fmt::Display| CaptureError::Other(e.to_string());
    let data = NSData::with_bytes(image);
    let request = unsafe { VNRecognizeTextRequest::init(VNRecognizeTextRequest::alloc()) };
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    request.setAutomaticallyDetectsLanguage(true);
    request.setRecognitionLanguages(&NSArray::from_retained_slice(&[NSString::from_str(
        "en-US",
    )]));

    let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        &data,
        &options,
    );
    handler
        .performRequests_error(NSArray::from_slice(&[request.as_super().as_super()]).as_ref())
        .map_err(|e| other(&e))?;

    let Some(results) = request.results() else {
        return Ok(String::new());
    };
    let mut boxes = Vec::new();
    for obs in results.iter() {
        let Some(candidate) = obs.topCandidates(1).iter().next() else {
            continue;
        };
        let bbox = unsafe { obs.boundingBox() };
        boxes.push(TextBox {
            x: bbox.origin.x,
            y: bbox.origin.y,
            width: bbox.size.width,
            height: bbox.size.height,
            text: candidate.string().to_string(),
        });
    }
    Ok(join_text_boxes(boxes))
}

/// Tiny 5×7 bitmap font so tests can render known words without a font file.
#[cfg(test)]
fn glyph(c: char) -> Option<[u8; 7]> {
    // Each byte is one row; the low 5 bits are pixels, MSB on the left of those 5.
    Some(match c {
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        '3' => [0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        _ => return None,
    })
}

#[cfg(test)]
fn render_label(text: &str) -> Vec<u8> {
    const SCALE: u32 = 12;
    const GAP: u32 = 2;
    const MARGIN: u32 = 80;
    let cols = text.chars().count() as u32;
    let w = MARGIN * 2 + cols * (5 + GAP) * SCALE;
    let h = MARGIN * 2 + 7 * SCALE;
    let mut rgba = vec![255u8; (w * h * 4) as usize]; // white
    for (i, c) in text.chars().enumerate() {
        let g = glyph(c).unwrap_or([0; 7]);
        let ox = MARGIN + i as u32 * (5 + GAP) * SCALE;
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5u32 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        let x = ox + col * SCALE + dx;
                        let y = MARGIN + row as u32 * SCALE + dy;
                        let p = ((y * w + x) * 4) as usize;
                        rgba[p..p + 3].fill(0); // black
                    }
                }
            }
        }
    }
    crate::capture::dedup::encode_jpeg(&rgba, w, h, 90).expect("jpeg")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::OcrEngine;
    use std::path::PathBuf;

    #[test]
    fn vision_ocr_reads_known_text() {
        let jpeg = render_label("HELLO TEST 42");
        let text = VisionOcr
            .recognize(&jpeg)
            .expect("Vision should return a result");
        let compact: String = text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        assert!(compact.contains("HELLO"), "expected HELLO in {text:?}");
        assert!(compact.contains("TEST"), "expected TEST in {text:?}");
        assert!(compact.contains("42"), "expected 42 in {text:?}");
    }

    fn walk_first_jpeg(dir: &PathBuf) -> Option<PathBuf> {
        let mut found = Vec::new();
        fn rec(dir: &PathBuf, found: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    rec(&p, found);
                } else if p
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("jpg"))
                    .unwrap_or(false)
                {
                    found.push(p);
                }
            }
        }
        rec(dir, &mut found);
        found.sort();
        found.into_iter().next()
    }

    #[test]
    #[ignore]
    fn vision_ocr_probe_saved_frame() {
        let dir = PathBuf::from(std::env::var("HOME").unwrap())
            .join("Library/Application Support/dev.anas4519.locallens/frames");
        let Some(jpeg) = walk_first_jpeg(&dir) else {
            eprintln!("no saved frames to probe at {}", dir.display());
            return;
        };
        let bytes = std::fs::read(&jpeg).unwrap();
        let text = VisionOcr
            .recognize(&bytes)
            .expect("Vision failed on a saved frame");
        let words = text.split_whitespace().count();
        let lines = if text.is_empty() {
            0
        } else {
            text.lines().count()
        };
        eprintln!(
            "probed {} ({} bytes) -> {} chars, {} words, {} lines",
            jpeg.display(),
            bytes.len(),
            text.len(),
            words,
            lines
        );
        assert!(!text.is_empty(), "expected some text on a real screenshot");
    }
}
