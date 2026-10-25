//! Cheap "did the screen change?" check. We shrink each frame to a tiny
//! grayscale thumbnail and count how many cells changed noticeably.
//! Small enough to run every 2s, sensitive enough to catch a new line of text.

pub const THUMB_W: u32 = 160;
pub const THUMB_H: u32 = 90;
/// Per-cell brightness delta (0-255) that counts as "changed".
const CELL_TOLERANCE: u8 = 12;
/// Fewer changed cells than this = treat as a duplicate frame.
pub const MIN_CHANGED_CELLS: usize = 20;

/// Grayscale thumbnail of a frame, `THUMB_W * THUMB_H` bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Thumb(pub Vec<u8>);

impl Thumb {
    /// Build from tightly packed RGBA pixels.
    pub fn from_rgba(rgba: &[u8], width: u32, height: u32) -> Option<Thumb> {
        let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
        let gray = image::DynamicImage::ImageRgba8(img).into_luma8();
        let small = image::imageops::resize(&gray, THUMB_W, THUMB_H, image::imageops::FilterType::Triangle);
        Some(Thumb(small.into_raw()))
    }

    pub fn changed_cells(&self, other: &Thumb) -> usize {
        self.0.iter().zip(&other.0).filter(|(a, b)| a.abs_diff(**b) > CELL_TOLERANCE).count()
    }

    /// `true` when `self` differs enough from `prev` to be worth saving.
    pub fn is_new(&self, prev: Option<&Thumb>) -> bool {
        match prev {
            None => true,
            Some(p) => self.changed_cells(p) >= MIN_CHANGED_CELLS,
        }
    }
}

/// Encode RGBA pixels as JPEG (alpha dropped). Quality 0-100.
pub fn encode_jpeg(rgba: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or("bad frame buffer size")?;
    let rgb = image::DynamicImage::ImageRgba8(img).into_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> Vec<u8> {
        (0..w * h).flat_map(|_| [v, v, v, 255]).collect()
    }

    #[test]
    fn identical_frames_are_duplicates() {
        let a = Thumb::from_rgba(&solid(320, 180, 40), 320, 180).unwrap();
        let b = Thumb::from_rgba(&solid(320, 180, 41), 320, 180).unwrap();
        assert!(a.is_new(None));
        assert!(!b.is_new(Some(&a)));
    }

    #[test]
    fn a_changed_region_is_new() {
        let (w, h) = (320, 180);
        let base = solid(w, h, 30);
        let mut changed = base.clone();
        // Paint a bright 40x20 block, like a new line of text.
        for y in 50..70 {
            for x in 100..140 {
                let i = ((y * w + x) * 4) as usize;
                changed[i..i + 3].copy_from_slice(&[230, 230, 230]);
            }
        }
        let a = Thumb::from_rgba(&base, w, h).unwrap();
        let b = Thumb::from_rgba(&changed, w, h).unwrap();
        assert!(b.is_new(Some(&a)));
    }

    #[test]
    fn jpeg_round_trips() {
        let jpg = encode_jpeg(&solid(64, 32, 128), 64, 32, 70).unwrap();
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        let back = image::load_from_memory(&jpg).unwrap();
        assert_eq!((back.width(), back.height()), (64, 32));
    }
}
