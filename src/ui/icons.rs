//! SVG icons resolved to physical pixels, without a second alpha multiplication.
use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, Vec2};
use resvg::{
    tiny_skia,
    usvg::{self, TreeParsing},
};

const SAMPLES: u32 = 4;

#[derive(Clone, Copy, Debug)]
struct PixelPlacement {
    origin: egui::Pos2,
    size: Vec2,
    phase: Vec2,
    width: u32,
    height: u32,
}
impl PixelPlacement {
    fn new(rect: Rect, dpi: f32) -> Self {
        let min = rect.min * dpi;
        let max = rect.max * dpi;
        let origin = egui::pos2(min.x.floor(), min.y.floor());
        Self {
            origin,
            size: rect.size() * dpi,
            phase: min - origin,
            width: (max.x.ceil() - origin.x) as u32,
            height: (max.y.ceil() - origin.y) as u32,
        }
    }
    fn rect(self, dpi: f32) -> Rect {
        Rect::from_min_size(
            self.origin / dpi,
            Vec2::new(self.width as f32, self.height as f32) / dpi,
        )
    }
    fn key(self) -> [u32; 6] {
        [
            self.width,
            self.height,
            self.size.x.to_bits(),
            self.size.y.to_bits(),
            self.phase.x.to_bits(),
            self.phase.y.to_bits(),
        ]
    }
}

fn rasterize(bytes: &[u8], placement: PixelPlacement) -> Result<ColorImage, String> {
    if placement.width == 0
        || placement.height == 0
        || placement.width > 256
        || placement.height > 256
    {
        return Err("Invalid icon size".into());
    }
    let tree =
        usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let mut pixmap = tiny_skia::Pixmap::new(placement.width * SAMPLES, placement.height * SAMPLES)
        .ok_or("Icon allocation failed")?;
    let sample = SAMPLES as f32;
    let transform = tiny_skia::Transform::from_row(
        placement.size.x / tree.size.width() * sample,
        0.0,
        0.0,
        placement.size.y / tree.size.height() * sample,
        placement.phase.x * sample,
        placement.phase.y * sample,
    );
    resvg::Tree::from_usvg(&tree).render(transform, &mut pixmap.as_mut());
    // tiny-skia pixels are premultiplied. Resolve all subpixels in that space;
    // interpreting them as straight RGBA creates dark fringes around strokes.
    let mut rgba = Vec::with_capacity((placement.width * placement.height * 4) as usize);
    for y in 0..placement.height {
        for x in 0..placement.width {
            let mut sum = [0_u32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let offset =
                        (((y * SAMPLES + sy) * pixmap.width() + x * SAMPLES + sx) * 4) as usize;
                    for (channel, value) in sum.iter_mut().enumerate() {
                        *value += pixmap.data()[offset + channel] as u32;
                    }
                }
            }
            let count = SAMPLES * SAMPLES;
            rgba.extend(sum.map(|v| ((v + count / 2) / count) as u8));
        }
    }
    Ok(ColorImage::from_rgba_premultiplied(
        [placement.width as usize, placement.height as usize],
        &rgba,
    ))
}

pub(super) fn paint(ui: &mut egui::Ui, rect: Rect, source: egui::ImageSource<'_>, tint: Color32) {
    let egui::ImageSource::Bytes { uri, bytes } = source else {
        return;
    };
    let dpi = ui.ctx().pixels_per_point();
    let placement = PixelPlacement::new(rect, dpi);
    let id = egui::Id::new(("physical-svg-icon", uri.as_ref(), placement.key()));
    let texture = ui.ctx().data(|d| d.get_temp::<TextureHandle>(id));
    let texture = texture.or_else(|| {
        let image = rasterize(bytes.as_ref(), placement).ok()?;
        let texture = ui
            .ctx()
            .load_texture(uri.as_ref(), image, egui::TextureOptions::NEAREST);
        ui.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
        Some(texture)
    });
    if let Some(texture) = texture {
        // Already antialiased and resolved for this exact pixel phase. A 1:1 blit
        // must not interpolate it again or move it to a different pixel phase.
        ui.painter().image(
            texture.id(),
            placement.rect(dpi),
            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            tint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_embedded_icon_renders_with_the_physical_pixel_pipeline() {
        for bytes in [
            include_bytes!("../../assets/brand_icon.svg").as_slice(),
            include_bytes!("../../assets/icon_layout_dashboard.svg").as_slice(),
            include_bytes!("../../assets/icon_settings.svg").as_slice(),
            include_bytes!("../../assets/icon_player_play.svg").as_slice(),
            include_bytes!("../../assets/icon_player_stop.svg").as_slice(),
            include_bytes!("../../assets/icon_external_link.svg").as_slice(),
            include_bytes!("../../assets/icon_refresh.svg").as_slice(),
            include_bytes!("../../assets/icon_server.svg").as_slice(),
            include_bytes!("../../assets/icon_browser.svg").as_slice(),
            include_bytes!("../../assets/icon_sparkles.svg").as_slice(),
            include_bytes!("../../assets/icon_world.svg").as_slice(),
            include_bytes!("../../assets/icon_folder.svg").as_slice(),
        ] {
            let placement = PixelPlacement::new(
                Rect::from_min_size(egui::pos2(13.0, 8.0), Vec2::splat(22.0)),
                1.25,
            );
            assert!(rasterize(bytes, placement)
                .unwrap()
                .pixels
                .iter()
                .any(|p| p.a() > 0));
        }
    }
    #[test]
    fn white_stroke_edges_stay_premultiplied_white_at_each_dpi() {
        for dpi in [1.0, 1.25, 1.5, 2.0] {
            let placement = PixelPlacement::new(
                Rect::from_min_size(egui::pos2(33.0, 72.0), Vec2::splat(21.0)),
                dpi,
            );
            let image =
                rasterize(include_bytes!("../../assets/icon_settings.svg"), placement).unwrap();
            assert!(image.pixels.iter().any(|p| p.a() > 0 && p.a() < 255));
            for p in image.pixels {
                assert_eq!(p.r(), p.a());
                assert_eq!(p.g(), p.a());
                assert_eq!(p.b(), p.a());
            }
        }
    }
    #[test]
    fn physical_bounds_cover_fractional_geometry_without_changing_its_phase() {
        let rect = Rect::from_min_size(egui::pos2(33.0, 72.0), Vec2::splat(21.0));
        let placement = PixelPlacement::new(rect, 1.25);
        assert_eq!([placement.width, placement.height], [27, 27]);
        assert_eq!(placement.phase, egui::vec2(0.25, 0.0));
        assert!(placement.rect(1.25).contains_rect(rect));
        let shifted = PixelPlacement::new(rect.translate(egui::vec2(0.4, 0.0)), 1.25);
        assert_ne!(placement.key(), shifted.key());
    }
}
