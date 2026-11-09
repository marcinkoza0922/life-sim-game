//! Images (icons, thumbnails, portraits) packed into one RGBA atlas, so they draw in the same
//! instanced call as shapes and text and stay correctly ordered with them.
//!
//! The atlas is never repacked: register images once at start-up (or when first needed), at
//! roughly the size they'll be shown. There are no mipmaps, so a big image drawn small aliases;
//! [`Images::add_png`] can downscale on load.

use glam::{Vec2, vec2};

pub const IMAGE_ATLAS_SIZE: u32 = 2048;
/// Each image is surrounded by a copy of its edge pixels, so linear filtering never reads a
/// neighbour.
const GUTTER: u32 = 1;

/// A registered image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(pub(crate) u32);

#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageEntry {
    /// Normalised atlas rect: u0, v0, u1, v1.
    pub uv: [f32; 4],
    /// Size in pixels.
    pub size: [u32; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImageError {
    /// Width or height is zero, or the pixel buffer doesn't match them.
    BadSize,
    /// No room left in the atlas.
    AtlasFull,
    Decode(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::BadSize => write!(f, "image size doesn't match its pixel data"),
            ImageError::AtlasFull => write!(f, "UI image atlas is full ({IMAGE_ATLAS_SIZE}x{IMAGE_ATLAS_SIZE})"),
            ImageError::Decode(e) => write!(f, "can't decode image: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

#[derive(Default)]
pub struct Images {
    /// Premultiplied RGBA, allocated on the first image.
    pub(crate) pixels: Vec<u8>,
    entries: Vec<ImageEntry>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// Rows touched since the last upload (y0..y1).
    pub(crate) dirty: Option<(u32, u32)>,
}

impl Images {
    /// Registers `rgba` (straight alpha, sRGB-encoded, row-major, `w * h * 4` bytes).
    pub fn add_rgba(&mut self, w: u32, h: u32, rgba: &[u8]) -> Result<ImageId, ImageError> {
        if w == 0 || h == 0 || rgba.len() != (w as usize) * (h as usize) * 4 {
            return Err(ImageError::BadSize);
        }
        let (gw, gh) = (w + 2 * GUTTER, h + 2 * GUTTER);
        let (ax, ay) = self.alloc(gw, gh).ok_or(ImageError::AtlasFull)?;
        if self.pixels.is_empty() {
            self.pixels = vec![0; (IMAGE_ATLAS_SIZE * IMAGE_ATLAS_SIZE * 4) as usize];
        }
        for gy in 0..gh {
            let sy = gy.saturating_sub(GUTTER).min(h - 1);
            for gx in 0..gw {
                let sx = gx.saturating_sub(GUTTER).min(w - 1);
                let s = ((sy * w + sx) * 4) as usize;
                let a = rgba[s + 3] as u32;
                let premul = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                let d = (((ay + gy) * IMAGE_ATLAS_SIZE + ax + gx) * 4) as usize;
                self.pixels[d..d + 4].copy_from_slice(&[premul(rgba[s]), premul(rgba[s + 1]), premul(rgba[s + 2]), a as u8]);
            }
        }
        self.dirty = Some(match self.dirty {
            Some((a, b)) => (a.min(ay), b.max(ay + gh)),
            None => (ay, ay + gh),
        });
        let s = IMAGE_ATLAS_SIZE as f32;
        let (x0, y0) = (ax + GUTTER, ay + GUTTER);
        self.entries.push(ImageEntry { uv: [x0 as f32 / s, y0 as f32 / s, (x0 + w) as f32 / s, (y0 + h) as f32 / s], size: [w, h] });
        Ok(ImageId(self.entries.len() as u32 - 1))
    }

    /// Decodes a PNG and registers it, first shrinking it (keeping the aspect ratio) so neither
    /// side exceeds `max_side` pixels, if given.
    pub fn add_png(&mut self, bytes: &[u8], max_side: Option<u32>) -> Result<ImageId, ImageError> {
        let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| ImageError::Decode(e.to_string()))?;
        let mut img = img.into_rgba8();
        if let Some(m) = max_side
            && img.width().max(img.height()) > m
        {
            let k = m as f32 / img.width().max(img.height()) as f32;
            let (w, h) = (((img.width() as f32 * k).round() as u32).max(1), ((img.height() as f32 * k).round() as u32).max(1));
            img = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
        }
        self.add_rgba(img.width(), img.height(), img.as_raw())
    }

    /// Size in pixels.
    pub fn size(&self, id: ImageId) -> Vec2 {
        let s = self.entries[id.0 as usize].size;
        vec2(s[0] as f32, s[1] as f32)
    }

    pub(crate) fn entry(&self, id: ImageId) -> ImageEntry {
        self.entries[id.0 as usize]
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > IMAGE_ATLAS_SIZE || h > IMAGE_ATLAS_SIZE {
            return None;
        }
        if self.shelf_x + w > IMAGE_ATLAS_SIZE {
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + h > IMAGE_ATLAS_SIZE {
            return None;
        }
        let at = (self.shelf_x, self.shelf_y);
        self.shelf_x += w;
        self.shelf_h = self.shelf_h.max(h);
        Some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(im: &Images, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * IMAGE_ATLAS_SIZE + x) * 4) as usize;
        im.pixels[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn premultiplies_and_pads_with_edge_pixels() {
        let mut im = Images::default();
        // 2x1: opaque red, half-transparent white
        let id = im.add_rgba(2, 1, &[255, 0, 0, 255, 255, 255, 255, 128]).unwrap();
        assert_eq!(im.size(id), vec2(2.0, 1.0));
        assert_eq!(px(&im, 1, 1), [255, 0, 0, 255]);
        assert_eq!(px(&im, 2, 1), [128, 128, 128, 128]);
        // gutter repeats the edges, corners included
        assert_eq!(px(&im, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&im, 3, 2), [128, 128, 128, 128]);
        let uv = im.entry(id).uv;
        let s = IMAGE_ATLAS_SIZE as f32;
        assert_eq!(uv, [1.0 / s, 1.0 / s, 3.0 / s, 2.0 / s]);
        assert_eq!(im.dirty, Some((0, 3)));
    }

    #[test]
    fn rejects_bad_sizes_and_overflow() {
        let mut im = Images::default();
        assert_eq!(im.add_rgba(2, 2, &[0; 12]), Err(ImageError::BadSize));
        assert_eq!(im.add_rgba(0, 0, &[]), Err(ImageError::BadSize));
        let big = vec![0u8; (IMAGE_ATLAS_SIZE * 4) as usize];
        assert_eq!(im.add_rgba(IMAGE_ATLAS_SIZE, 1, &big), Err(ImageError::AtlasFull), "gutter doesn't fit");
        let half = IMAGE_ATLAS_SIZE / 2 - 2;
        let block = vec![255u8; (half * half * 4) as usize];
        let ids: Vec<_> = (0..4).map(|_| im.add_rgba(half, half, &block).unwrap()).collect();
        assert_eq!(ids.len(), 4);
        assert_eq!(im.add_rgba(half, half, &block), Err(ImageError::AtlasFull));
    }

    #[test]
    fn decodes_and_shrinks_png() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(64, 32, image::Rgba([10, 20, 30, 255]));
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut im = Images::default();
        let id = im.add_png(&png, Some(16)).unwrap();
        assert_eq!(im.size(id), vec2(16.0, 8.0));
        assert!(matches!(im.add_png(b"nope", None), Err(ImageError::Decode(_))));
    }
}
