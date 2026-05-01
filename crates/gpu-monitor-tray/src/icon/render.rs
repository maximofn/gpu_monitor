use std::path::Path;

use ab_glyph::{point, Font, FontArc, Glyph, PxScale, ScaleFont};
use anyhow::{anyhow, Context, Result};
use gpu_monitor_core::Gpu;
use image::ImageReader;
use tiny_skia::{
    BlendMode, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Transform,
};

const PER_GPU_GAP: u32 = 4;
const DONUT_PADDING: u32 = 2;
const DEFAULT_FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
];

const COLOR_FREE: [u8; 4] = [0x66, 0xb3, 0xff, 0xff];
const COLOR_OK: [u8; 4] = [0x99, 0xff, 0x99, 0xff];
const COLOR_WARN1: [u8; 4] = [0xff, 0xdb, 0x4d, 0xff];
const COLOR_WARN2: [u8; 4] = [0xff, 0xcc, 0x99, 0xff];
const COLOR_HIGH: [u8; 4] = [0xff, 0x66, 0x66, 0xff];
const COLOR_TEXT: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

pub struct RenderedIcon {
    pub width: i32,
    pub height: i32,
    /// Bytes in `ARGB32` (network byte order: A, R, G, B per pixel),
    /// the layout that StatusNotifierItem expects for `IconPixmap`.
    pub argb: Vec<u8>,
}

pub struct IconRenderer {
    height: u32,
    base_icon: Option<Pixmap>,
    font: FontArc,
}

struct BlockLayout {
    donut_size: u32,
    icon_w: u32,
    text_w: u32,
    connected: bool,
}

impl IconRenderer {
    pub fn new(height: u32, base_icon_path: &Path) -> Result<Self> {
        let base_icon = load_base_icon(base_icon_path, height).ok();
        let font = load_font().context("loading DejaVu Sans Mono font")?;
        Ok(Self { height, base_icon, font })
    }

    pub fn render(&self, gpus: &[Gpu], connected: bool) -> RenderedIcon {
        let pixmap = self.render_pixmap(gpus, connected);
        RenderedIcon {
            width: pixmap.width() as i32,
            height: pixmap.height() as i32,
            argb: rgba_premul_to_argb_straight(pixmap.data()),
        }
    }

    /// Render and encode the icon as a PNG (straight RGBA, decoders happy).
    pub fn render_png(&self, gpus: &[Gpu], connected: bool) -> Result<Vec<u8>> {
        let pixmap = self.render_pixmap(gpus, connected);
        let straight = unpremultiply_to_rgba(pixmap.data());
        let img = image::RgbaImage::from_raw(pixmap.width(), pixmap.height(), straight)
            .ok_or_else(|| anyhow!("failed to wrap pixmap as RgbaImage"))?;
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Png,
        )?;
        Ok(buf)
    }

    fn render_pixmap(&self, gpus: &[Gpu], connected: bool) -> Pixmap {
        let h = self.height;
        let donut_size = h.saturating_sub(DONUT_PADDING * 2).max(8);
        let text_w = self.measure_text("GPU 0(00C)", text_size(h));
        let icon_w = self.base_icon.as_ref().map(|p| p.width()).unwrap_or(0);
        let per_gpu_w = icon_w + 2 + text_w + 2 + donut_size;
        let count = gpus.len().max(1) as u32;
        let total_w = per_gpu_w * count + PER_GPU_GAP * count.saturating_sub(1);

        let mut pixmap = Pixmap::new(total_w.max(h), h).expect("non-zero pixmap");

        let layout = BlockLayout { donut_size, icon_w, text_w, connected };
        let mut x_cursor = 0u32;
        for (i, gpu) in gpus.iter().enumerate() {
            if i > 0 {
                x_cursor += PER_GPU_GAP;
            }
            self.draw_gpu_block(&mut pixmap, x_cursor, gpu, &layout);
            x_cursor += per_gpu_w;
        }

        if gpus.is_empty() {
            self.draw_empty(&mut pixmap, donut_size);
        }

        pixmap
    }

    fn draw_gpu_block(&self, pixmap: &mut Pixmap, x: u32, gpu: &Gpu, layout: &BlockLayout) {
        if let Some(ref icon) = self.base_icon {
            let icon_y = (self.height as i32 - icon.height() as i32) / 2;
            pixmap.draw_pixmap(
                x as i32,
                icon_y,
                icon.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        let temp = gpu.temperature_c.unwrap_or(0);
        let label = format!("GPU {}({:>2}C)", gpu.index, temp);
        let text_x = x + layout.icon_w + 2;
        let text_color = if layout.connected {
            COLOR_TEXT
        } else {
            [0xaa, 0xaa, 0xaa, 0xff]
        };
        self.draw_text(pixmap, text_x as f32, &label, text_size(self.height), text_color);

        let donut_x = (x + layout.icon_w + 2 + layout.text_w + 2) as f32;
        let used_pct = gpu.memory.used_percent();
        draw_donut(
            pixmap,
            donut_x,
            DONUT_PADDING as f32,
            layout.donut_size,
            used_pct,
            layout.connected,
        );
    }

    fn draw_empty(&self, pixmap: &mut Pixmap, donut_size: u32) {
        self.draw_text(pixmap, 0.0, "no GPUs", text_size(self.height), COLOR_TEXT);
        draw_donut(pixmap, 0.0, DONUT_PADDING as f32, donut_size, 0.0, false);
    }

    fn measure_text(&self, text: &str, px: f32) -> u32 {
        let scaled = self.font.as_scaled(PxScale::from(px));
        let width: f32 = text.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum();
        width.ceil() as u32
    }

    fn draw_text(&self, pixmap: &mut Pixmap, x: f32, text: &str, px: f32, color: [u8; 4]) {
        let scaled = self.font.as_scaled(PxScale::from(px));
        let ascent = scaled.ascent();
        let baseline_y = ((self.height as f32 - px) / 2.0) + ascent;
        let mut cursor_x = x;
        for ch in text.chars() {
            let glyph: Glyph = scaled.scaled_glyph(ch);
            let positioned = Glyph { position: point(cursor_x, baseline_y), ..glyph };
            cursor_x += scaled.h_advance(positioned.id);
            if let Some(outlined) = scaled.outline_glyph(positioned) {
                let bb = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    let px_x = bb.min.x as i32 + gx as i32;
                    let px_y = bb.min.y as i32 + gy as i32;
                    if px_x < 0 || px_y < 0 {
                        return;
                    }
                    // Gamma 2.0 boost: thin strokes at small sizes spread their
                    // coverage across multiple pixels and otherwise look faint.
                    let boosted = coverage.sqrt();
                    blend_pixel(pixmap, px_x as u32, px_y as u32, color, boosted);
                });
            }
        }
    }
}

fn text_size(h: u32) -> f32 {
    (h as f32 * 0.55).clamp(8.0, 16.0)
}

fn load_base_icon(path: &Path, target_h: u32) -> Result<Pixmap> {
    let img = ImageReader::open(path)
        .with_context(|| format!("opening icon {}", path.display()))?
        .decode()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let scale = target_h as f32 / h as f32;
    let new_w = ((w as f32) * scale).round().max(1.0) as u32;
    let new_h = target_h;
    let resized = image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Lanczos3);
    let mut pixmap = Pixmap::new(new_w, new_h).context("alloc pixmap")?;
    let dst = pixmap.data_mut();
    for (chunk, out) in resized.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        let a = chunk[3] as u32;
        out[0] = (chunk[0] as u32 * a / 255) as u8;
        out[1] = (chunk[1] as u32 * a / 255) as u8;
        out[2] = (chunk[2] as u32 * a / 255) as u8;
        out[3] = a as u8;
    }
    Ok(pixmap)
}

fn load_font() -> Result<FontArc> {
    for path in DEFAULT_FONT_PATHS {
        if let Ok(bytes) = std::fs::read(path) {
            return Ok(FontArc::try_from_vec(bytes)?);
        }
    }
    anyhow::bail!(
        "DejaVu Sans Mono font not found in any of: {:?}; install fonts-dejavu-core",
        DEFAULT_FONT_PATHS
    );
}

fn used_color(pct: f32) -> [u8; 4] {
    if pct >= 90.0 {
        COLOR_HIGH
    } else if pct >= 80.0 {
        COLOR_WARN2
    } else if pct >= 70.0 {
        COLOR_WARN1
    } else {
        COLOR_OK
    }
}

fn draw_donut(pixmap: &mut Pixmap, x: f32, y: f32, size: u32, used_pct: f32, connected: bool) {
    let cx = x + size as f32 / 2.0;
    let cy = y + size as f32 / 2.0;
    let r_outer = size as f32 / 2.0;
    let r_inner = r_outer * 0.55;

    let free_color = if connected { COLOR_FREE } else { [0x80, 0x80, 0x80, 0xff] };
    fill_disk(pixmap, cx, cy, r_outer, free_color);

    if used_pct > 0.5 {
        let color = if connected { used_color(used_pct) } else { [0x60, 0x60, 0x60, 0xff] };
        let sweep = (used_pct.clamp(0.0, 100.0) / 100.0) * 360.0;
        fill_pie(pixmap, cx, cy, r_outer, -90.0, -90.0 + sweep, color);
    }

    clear_disk(pixmap, cx, cy, r_inner);
}

fn fill_disk(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, color: [u8; 4]) {
    let path = match PathBuilder::from_circle(cx, cy, r) {
        Some(p) => p,
        None => return,
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
}

fn clear_disk(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32) {
    let path = match PathBuilder::from_circle(cx, cy, r) {
        Some(p) => p,
        None => return,
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(0, 0, 0, 0);
    paint.blend_mode = BlendMode::Clear;
    pixmap.fill_path(&path, &paint, FillRule::EvenOdd, Transform::identity(), None);
}

fn fill_pie(
    pixmap: &mut Pixmap,
    cx: f32,
    cy: f32,
    r: f32,
    start_deg: f32,
    end_deg: f32,
    color: [u8; 4],
) {
    let segments = ((end_deg - start_deg).abs() / 5.0).ceil().max(2.0) as u32;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = (start_deg + t * (end_deg - start_deg)).to_radians();
        pb.line_to(cx + r * angle.cos(), cy + r * angle.sin());
    }
    pb.close();
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn blend_pixel(pixmap: &mut Pixmap, x: u32, y: u32, color: [u8; 4], coverage: f32) {
    if x >= pixmap.width() || y >= pixmap.height() {
        return;
    }
    let stride = pixmap.width() as usize * 4;
    let idx = (y as usize) * stride + (x as usize) * 4;
    let data = pixmap.data_mut();
    let src_a = (coverage.clamp(0.0, 1.0) * color[3] as f32) as u32;
    if src_a == 0 {
        return;
    }
    let inv_a = 255 - src_a;
    let blend = |s: u8, d: u8| -> u8 {
        ((s as u32 * src_a + d as u32 * inv_a) / 255) as u8
    };
    data[idx] = blend(color[0], data[idx]);
    data[idx + 1] = blend(color[1], data[idx + 1]);
    data[idx + 2] = blend(color[2], data[idx + 2]);
    data[idx + 3] = (data[idx + 3] as u32 + src_a).min(255) as u8;
}

/// Convert tiny-skia's premultiplied RGBA into straight RGBA bytes
/// (so the result encodes as a normal PNG that any image viewer renders).
fn unpremultiply_to_rgba(rgba: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; rgba.len()];
    for (chunk, slot) in rgba.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let a = chunk[3];
        if a == 0 {
            slot.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            let unpremul = |c: u8| -> u8 {
                let v = (c as u32 * 255 + a as u32 / 2) / a as u32;
                v.min(255) as u8
            };
            slot[0] = unpremul(chunk[0]);
            slot[1] = unpremul(chunk[1]);
            slot[2] = unpremul(chunk[2]);
            slot[3] = a;
        }
    }
    out
}

/// Convert tiny-skia's premultiplied RGBA to the ARGB32 network-byte-order
/// layout that StatusNotifierItem expects (alpha as straight, not premultiplied).
fn rgba_premul_to_argb_straight(rgba: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; rgba.len()];
    for (chunk, slot) in rgba.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let a = chunk[3];
        slot[0] = a;
        if a == 0 {
            slot[1] = 0;
            slot[2] = 0;
            slot[3] = 0;
        } else {
            let unpremul = |c: u8| -> u8 {
                let v = (c as u32 * 255 + a as u32 / 2) / a as u32;
                v.min(255) as u8
            };
            slot[1] = unpremul(chunk[0]);
            slot[2] = unpremul(chunk[1]);
            slot[3] = unpremul(chunk[2]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_color_thresholds() {
        assert_eq!(used_color(0.0), COLOR_OK);
        assert_eq!(used_color(69.9), COLOR_OK);
        assert_eq!(used_color(70.0), COLOR_WARN1);
        assert_eq!(used_color(80.0), COLOR_WARN2);
        assert_eq!(used_color(90.0), COLOR_HIGH);
        assert_eq!(used_color(100.0), COLOR_HIGH);
    }

    #[test]
    fn opaque_pixels_passthrough_color() {
        // Premul == straight when alpha == 255.
        let rgba = vec![0x11, 0x22, 0x33, 0xff];
        let argb = rgba_premul_to_argb_straight(&rgba);
        assert_eq!(argb, vec![0xff, 0x11, 0x22, 0x33]);
    }

    #[test]
    fn fully_transparent_pixels_become_zero() {
        let rgba = vec![0x33, 0x33, 0x33, 0x00];
        let argb = rgba_premul_to_argb_straight(&rgba);
        assert_eq!(argb, vec![0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn half_alpha_pixel_is_unpremultiplied() {
        // Solid green at 50% alpha is stored premultiplied as (0, 128, 0, 128).
        // We want it to come out as straight (alpha=128, R=0, G=255, B=0)
        // so the compositor can blend it onto whatever panel background.
        let rgba = vec![0x00, 0x80, 0x00, 0x80];
        let argb = rgba_premul_to_argb_straight(&rgba);
        assert_eq!(argb[0], 0x80);
        assert!(argb[2] >= 0xfe, "green should round up to ~255, got {:#x}", argb[2]);
    }
}
