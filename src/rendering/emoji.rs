use std::collections::HashMap;
use std::path::Path;

use femtovg::renderer::OpenGl;
use femtovg::{Canvas, ImageFlags, ImageId, Paint, Path as VectorPath};
use rgb::FromSlice;
use swash::FontRef;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};

use crate::model::Rect;

const RASTER_SIZE: f32 = 224.0;
const INK_FRACTION: f32 = 0.9;

pub(super) struct EmojiRenderer {
    font_data: Option<Vec<u8>>,
    scale_context: ScaleContext,
    cache: HashMap<String, CachedGlyph>,
}

struct CachedGlyph {
    image: ImageId,
    width: u32,
    height: u32,
}

impl EmojiRenderer {
    pub fn new() -> Self {
        Self {
            font_data: None,
            scale_context: ScaleContext::new(),
            cache: HashMap::new(),
        }
    }

    /// Selects the first loaded font that can produce a real colour emoji.
    pub fn try_load_font(&mut self, path: &Path) {
        if self.font_data.is_some() {
            return;
        }
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let mut probe_context = ScaleContext::new();
        if rasterize_color_glyph(&mut probe_context, &data, '\u{1f600}').is_some() {
            self.font_data = Some(data);
        }
    }

    pub fn paint(
        &mut self,
        canvas: &mut Canvas<OpenGl>,
        glyph: &str,
        bounds: Rect,
        opacity: f32,
    ) -> bool {
        if !self.cache.contains_key(glyph) && !self.cache_glyph(canvas, glyph) {
            return false;
        }
        let Some(cached) = self.cache.get(glyph) else {
            return false;
        };
        let max_dimension = cached.width.max(cached.height) as f32;
        if max_dimension <= 0.0 {
            return false;
        }
        let scale = bounds.width().min(bounds.height()) * INK_FRACTION / max_dimension;
        let width = cached.width as f32 * scale;
        let height = cached.height as f32 * scale;
        let left = bounds.center().x - width * 0.5;
        let top = bounds.center().y - height * 0.5;
        let mut path = VectorPath::new();
        path.rect(left, top, width, height);
        canvas.fill_path(
            &path,
            &Paint::image(
                cached.image,
                left,
                top,
                width,
                height,
                0.0,
                opacity.clamp(0.0, 1.0),
            ),
        );
        true
    }

    fn cache_glyph(&mut self, canvas: &mut Canvas<OpenGl>, glyph: &str) -> bool {
        let Some(character) = glyph
            .chars()
            .find(|character| !matches!(character, '\u{fe0e}' | '\u{fe0f}'))
        else {
            return false;
        };
        let Some(font_data) = self.font_data.as_deref() else {
            return false;
        };
        let Some(bitmap) = rasterize_color_glyph(&mut self.scale_context, font_data, character)
        else {
            return false;
        };
        let width = bitmap.placement.width;
        let height = bitmap.placement.height;
        if width == 0 || height == 0 || bitmap.data.len() != width as usize * height as usize * 4 {
            return false;
        }
        let pixels = imgref::Img::new(bitmap.data.as_rgba(), width as usize, height as usize);
        let flags = ImageFlags::GENERATE_MIPMAPS | ImageFlags::PREMULTIPLIED;
        let Ok(image) = canvas.create_image(pixels, flags) else {
            return false;
        };
        self.cache.insert(
            glyph.to_owned(),
            CachedGlyph {
                image,
                width,
                height,
            },
        );
        true
    }
}

fn rasterize_color_glyph(
    context: &mut ScaleContext,
    font_data: &[u8],
    character: char,
) -> Option<swash::scale::image::Image> {
    let font = FontRef::from_index(font_data, 0)?;
    let glyph = font.charmap().map(character);
    if glyph == 0 {
        return None;
    }
    let mut scaler = context.builder(font).size(RASTER_SIZE).hint(true).build();
    let image = Render::new(&[
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
    ])
    .render(&mut scaler, glyph)?;
    (image.content == Content::Color).then_some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_font_or_non_emoji_data_is_rejected() {
        let mut context = ScaleContext::new();
        assert!(rasterize_color_glyph(&mut context, b"not a font", '\u{1f600}').is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_system_emoji_font_produces_color_pixels() {
        let path = std::path::PathBuf::from(std::env::var_os("WINDIR").expect("WINDIR"))
            .join("Fonts")
            .join("seguiemj.ttf");
        let data = std::fs::read(path).expect("Segoe UI Emoji font");
        let mut context = ScaleContext::new();
        let image =
            rasterize_color_glyph(&mut context, &data, '\u{1f600}').expect("color emoji glyph");

        assert_eq!(image.content, Content::Color);
        assert!(image.placement.width > 0);
        assert!(image.placement.height > 0);
        assert!(
            image
                .data
                .chunks_exact(4)
                .any(|pixel| { pixel[3] != 0 && (pixel[0] != pixel[1] || pixel[1] != pixel[2]) })
        );
    }
}
