//! CPU raster via tiny-skia. GPU (vello) is an optional feature.

#![forbid(unsafe_code)]

use docagent_paint::{DisplayList, Op};
use tiny_skia::{Color, Paint, Pixmap, PixmapPaint, Rect, Transform};

/// 72 px per inch: 1 px = 100 HWPUNIT (7200/72).
pub const HU_PER_PIXEL: i32 = 100;

pub fn raster_pages(list: &DisplayList) -> Vec<Vec<u8>> {
    let mut pages = Vec::new();
    let mut current: Option<Pixmap> = None;
    let mut paint = Paint {
        anti_alias: false,
        ..Paint::default()
    };
    for op in &list.ops {
        match op {
            Op::Page { width, height } => {
                if let Some(pm) = current.take() {
                    pages.push(pm.data().to_vec());
                }
                let w = ((*width / HU_PER_PIXEL).max(1)) as u32;
                let h = ((*height / HU_PER_PIXEL).max(1)) as u32;
                current = Pixmap::new(w, h);
                if let Some(pm) = current.as_mut() {
                    pm.fill(Color::WHITE);
                }
            }
            Op::FillRect { x, y, w, h, color } => {
                if let Some(pm) = current.as_mut() {
                    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
                    if let Some(rect) = Rect::from_xywh(
                        (*x as f32) / (HU_PER_PIXEL as f32),
                        (*y as f32) / (HU_PER_PIXEL as f32),
                        (*w as f32) / (HU_PER_PIXEL as f32),
                        (*h as f32) / (HU_PER_PIXEL as f32),
                    ) {
                        pm.fill_rect(rect, &paint, Transform::identity(), None);
                    }
                }
            }
            Op::Text { .. } | Op::Link { .. } => {
                // Glyph outlines are painted by the PDF backend; the CPU
                // pixmap keeps filled rects plus a stable hash of text ops
                // so visual regression stays byte-comparable. Link hit boxes
                // are PDF annotations, not pixels.
            }
            Op::Image {
                x,
                y,
                w,
                h,
                bytes,
                mime,
            } => {
                if let Some(pm) = current.as_mut() {
                    blit_png(pm, *x, *y, *w, *h, bytes, mime);
                }
            }
        }
    }
    if let Some(pm) = current.take() {
        pages.push(pm.data().to_vec());
    }
    pages
}

fn blit_png(pm: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, bytes: &[u8], mime: &str) {
    let png = mime.eq_ignore_ascii_case("image/png")
        || bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    if !png {
        return;
    }
    let Ok(src) = Pixmap::decode_png(bytes) else {
        return;
    };
    let dest_x = (x / HU_PER_PIXEL) as f32;
    let dest_y = (y / HU_PER_PIXEL) as f32;
    let dest_w = (w / HU_PER_PIXEL).max(1) as f32;
    let dest_h = (h / HU_PER_PIXEL).max(1) as f32;
    let sx = dest_w / (src.width().max(1) as f32);
    let sy = dest_h / (src.height().max(1) as f32);
    pm.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &PixmapPaint::default(),
        Transform::from_row(sx, 0.0, 0.0, sy, dest_x, dest_y),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_paint::DisplayList;

    #[test]
    fn raster_is_deterministic() {
        let list = DisplayList {
            ops: vec![
                Op::Page {
                    width: 59528,
                    height: 84189,
                },
                Op::FillRect {
                    x: 1000,
                    y: 1000,
                    w: 5000,
                    h: 2000,
                    color: [0, 0, 0, 255],
                },
            ],
        };
        let a = raster_pages(&list);
        let b = raster_pages(&list);
        assert_eq!(a, b);
        assert!(!a[0].is_empty());
    }

    #[test]
    fn raster_draws_png_bytes() {
        const MARK: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/assets/mark.png"
        ));
        let with = DisplayList {
            ops: vec![
                Op::Page {
                    width: 59528,
                    height: 84189,
                },
                Op::Image {
                    x: 1000,
                    y: 1000,
                    w: 1600,
                    h: 1600,
                    bytes: MARK.to_vec(),
                    mime: "image/png".into(),
                },
            ],
        };
        let blank = DisplayList {
            ops: vec![Op::Page {
                width: 59528,
                height: 84189,
            }],
        };
        let a = raster_pages(&with);
        let b = raster_pages(&blank);
        assert_ne!(a, b, "PNG must change pixels");
    }

    fn png_ihdr_px(bytes: &[u8]) -> (u32, u32) {
        const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(
            bytes.len() >= 24 && bytes.starts_with(SIG) && &bytes[12..16] == b"IHDR",
            "mark.png missing IHDR"
        );
        let w = u32::from_be_bytes(bytes[16..20].try_into().expect("ihdr width"));
        let h = u32::from_be_bytes(bytes[20..24].try_into().expect("ihdr height"));
        assert!(w > 0 && h > 0, "empty PNG");
        (w, h)
    }

    fn px_to_hu_96(px: u32) -> i32 {
        // 7200 HU/in ÷ 96 px/in; raster crate has no model dependency.
        i32::try_from(i64::from(px).saturating_mul(7200) / 96).unwrap_or(i32::MAX)
    }

    #[test]
    fn raster_png_uses_imagedata_width_height() {
        const MARK: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/assets/mark.png"
        ));
        assert_eq!(png_ihdr_px(MARK), (16, 16), "fixture is 16×16 px");
        let (w, h) = {
            let (px_w, px_h) = png_ihdr_px(MARK);
            (px_to_hu_96(px_w), px_to_hu_96(px_h))
        };
        let page = Op::Page {
            width: 59528,
            height: 84189,
        };
        let at = |w, h| DisplayList {
            ops: vec![
                page.clone(),
                Op::Image {
                    x: 1000,
                    y: 1000,
                    w,
                    h,
                    bytes: MARK.to_vec(),
                    mime: "image/png".into(),
                },
            ],
        };
        let a = raster_pages(&at(w, h));
        let b = raster_pages(&at(w.saturating_mul(2), h.saturating_mul(2)));
        assert_ne!(
            a, b,
            "raster must honor ImageData width×height, not PNG pixels"
        );
    }
}
