use anyhow::{Context, Result, anyhow};
use femtovg::renderer::OpenGl;
use femtovg::{Canvas, Color, ImageFlags, ImageId, Paint, Path};
use imgref::Img;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;
use rgb::RGBA8;

use crate::model::{OverlayAction, Rect, Tool};

const ICON_RASTER_SIZE: u32 = 96;
const ICON_MAX_SIZE: f32 = 18.0;
const ICON_SIZE_RATIO: f32 = 0.8;
const ICON_IMAGE_FLAGS: ImageFlags = ImageFlags::GENERATE_MIPMAPS;

const SQUARE: &[u8] = include_bytes!("../../assets/icons/square.svg");
const CIRCLE: &[u8] = include_bytes!("../../assets/icons/circle.svg");
const EMOTION: &[u8] = include_bytes!("../../assets/icons/emotion.svg");
const ARROW: &[u8] = include_bytes!("../../assets/icons/move-up-right.svg");
const PEN: &[u8] = include_bytes!("../../assets/icons/pen.svg");
const MOSAIC: &[u8] = include_bytes!("../../assets/icons/mosaic.svg");
const TEXT: &[u8] = include_bytes!("../../assets/icons/type.svg");
const UNDO: &[u8] = include_bytes!("../../assets/icons/undo-2.svg");
const EXTRACT_TEXT: &[u8] = include_bytes!("../../assets/icons/extracttext.svg");
const SCROLL_CAPTURE: &[u8] = include_bytes!("../../assets/icons/scroll-capture.svg");
const LANGUAGES: &[u8] = include_bytes!("../../assets/icons/languages.svg");
const SAVE: &[u8] = include_bytes!("../../assets/icons/download.svg");
const PIN: &[u8] = include_bytes!("../../assets/icons/pin.svg");
const CONFIRM: &[u8] = include_bytes!("../../assets/icons/check.svg");
const CANCEL: &[u8] = include_bytes!("../../assets/icons/x.svg");
const SELECT: &[u8] = include_bytes!("../../assets/icons/edit.svg");
const FILL: &[u8] = include_bytes!("../../assets/icons/filling.svg");

pub(super) struct ToolbarIcons {
    items: [ImageId; ToolbarIcon::COUNT],
    fill_option: ImageId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum ToolbarIcon {
    Rectangle,
    Circle,
    Emotion,
    Arrow,
    Pen,
    Mosaic,
    Text,
    Undo,
    ExtractText,
    ScrollCapture,
    Languages,
    Save,
    Pin,
    Confirm,
    Cancel,
    Select,
}

impl ToolbarIcon {
    const COUNT: usize = 16;
    const ALL: [Self; Self::COUNT] = [
        Self::Rectangle,
        Self::Circle,
        Self::Emotion,
        Self::Arrow,
        Self::Pen,
        Self::Mosaic,
        Self::Text,
        Self::Undo,
        Self::ExtractText,
        Self::ScrollCapture,
        Self::Languages,
        Self::Save,
        Self::Pin,
        Self::Confirm,
        Self::Cancel,
        Self::Select,
    ];

    const fn source(self) -> &'static [u8] {
        match self {
            Self::Rectangle => SQUARE,
            Self::Circle => CIRCLE,
            Self::Emotion => EMOTION,
            Self::Arrow => ARROW,
            Self::Pen => PEN,
            Self::Mosaic => MOSAIC,
            Self::Text => TEXT,
            Self::Undo => UNDO,
            Self::ExtractText => EXTRACT_TEXT,
            Self::ScrollCapture => SCROLL_CAPTURE,
            Self::Languages => LANGUAGES,
            Self::Save => SAVE,
            Self::Pin => PIN,
            Self::Confirm => CONFIRM,
            Self::Cancel => CANCEL,
            Self::Select => SELECT,
        }
    }

    #[cfg(test)]
    const fn asset_name(self) -> &'static str {
        match self {
            Self::Rectangle => "square.svg",
            Self::Circle => "circle.svg",
            Self::Emotion => "emotion.svg",
            Self::Arrow => "move-up-right.svg",
            Self::Pen => "pen.svg",
            Self::Mosaic => "mosaic.svg",
            Self::Text => "type.svg",
            Self::Undo => "undo-2.svg",
            Self::ExtractText => "extracttext.svg",
            Self::ScrollCapture => "scroll-capture.svg",
            Self::Languages => "languages.svg",
            Self::Save => "download.svg",
            Self::Pin => "pin.svg",
            Self::Confirm => "check.svg",
            Self::Cancel => "x.svg",
            Self::Select => "edit.svg",
        }
    }
}

impl ToolbarIcons {
    pub fn new(canvas: &mut Canvas<OpenGl>) -> Result<Self> {
        let mut items = Vec::with_capacity(ToolbarIcon::COUNT);
        for icon in ToolbarIcon::ALL {
            items.push(upload_svg(canvas, icon.source())?);
        }
        let items = items
            .try_into()
            .map_err(|_| anyhow!("toolbar icon table has the wrong length"))?;
        Ok(Self {
            items,
            fill_option: upload_svg(canvas, FILL)?,
        })
    }

    pub fn paint(
        &self,
        canvas: &mut Canvas<OpenGl>,
        action: OverlayAction,
        bounds: Rect,
        color: Color,
    ) {
        let Some(icon) = icon_for_action(action) else {
            return;
        };
        paint_image(canvas, self.items[icon as usize], bounds, color);
    }

    pub fn paint_fill(&self, canvas: &mut Canvas<OpenGl>, bounds: Rect, color: Color) {
        paint_image(canvas, self.fill_option, bounds, color);
    }
}

fn upload_svg(canvas: &mut Canvas<OpenGl>, source: &[u8]) -> Result<ImageId> {
    let pixels = rasterize_svg(source)?;
    canvas
        .create_image(
            Img::new(
                pixels.as_slice(),
                ICON_RASTER_SIZE as usize,
                ICON_RASTER_SIZE as usize,
            ),
            ICON_IMAGE_FLAGS,
        )
        .map_err(|error| anyhow!("failed to upload toolbar SVG: {error:?}"))
}

fn rasterize_svg(source: &[u8]) -> Result<Vec<RGBA8>> {
    let tree = usvg::Tree::from_data(source, &usvg::Options::default())
        .context("failed to parse toolbar SVG")?;
    let size = tree.size();
    let scale =
        (ICON_RASTER_SIZE as f32 / size.width()).min(ICON_RASTER_SIZE as f32 / size.height());
    let x = (ICON_RASTER_SIZE as f32 - size.width() * scale) * 0.5;
    let y = (ICON_RASTER_SIZE as f32 - size.height() * scale) * 0.5;
    let mut pixmap = Pixmap::new(ICON_RASTER_SIZE, ICON_RASTER_SIZE)
        .ok_or_else(|| anyhow!("failed to allocate toolbar SVG pixmap"))?;
    let transform = Transform::from_scale(scale, scale).post_translate(x, y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // The original assets are monochrome. A white, straight-alpha mask lets
    // FemtoVG tint one cached texture for normal, active and disabled states.
    Ok(pixmap
        .data()
        .chunks_exact(4)
        .map(|pixel| RGBA8::new(255, 255, 255, pixel[3]))
        .collect())
}

fn paint_image(canvas: &mut Canvas<OpenGl>, image: ImageId, bounds: Rect, tint: Color) {
    let bounds = icon_bounds(bounds);
    let size = bounds.width();
    if size <= 0.0 {
        return;
    }
    let mut icon = Path::new();
    icon.rect(bounds.left, bounds.top, size, size);
    canvas.fill_path(
        &icon,
        &Paint::image_tint(image, bounds.left, bounds.top, size, size, 0.0, tint),
    );
}

fn icon_bounds(bounds: Rect) -> Rect {
    let size = (bounds.width().min(bounds.height()) * ICON_SIZE_RATIO).clamp(0.0, ICON_MAX_SIZE);
    let center = bounds.center();
    Rect::new(
        center.x - size * 0.5,
        center.y - size * 0.5,
        center.x + size * 0.5,
        center.y + size * 0.5,
    )
}

fn icon_for_action(action: OverlayAction) -> Option<ToolbarIcon> {
    match action {
        OverlayAction::Tool(Tool::Rectangle) => Some(ToolbarIcon::Rectangle),
        OverlayAction::Tool(Tool::Circle) => Some(ToolbarIcon::Circle),
        OverlayAction::Tool(Tool::Emotion) => Some(ToolbarIcon::Emotion),
        OverlayAction::Tool(Tool::Arrow) => Some(ToolbarIcon::Arrow),
        OverlayAction::Tool(Tool::Pen) => Some(ToolbarIcon::Pen),
        OverlayAction::Tool(Tool::Mosaic) => Some(ToolbarIcon::Mosaic),
        OverlayAction::Tool(Tool::Text) => Some(ToolbarIcon::Text),
        OverlayAction::Undo => Some(ToolbarIcon::Undo),
        OverlayAction::ExtractText => Some(ToolbarIcon::ExtractText),
        OverlayAction::ScrollCapture => Some(ToolbarIcon::ScrollCapture),
        OverlayAction::Languages => Some(ToolbarIcon::Languages),
        OverlayAction::Save => Some(ToolbarIcon::Save),
        OverlayAction::Pin => Some(ToolbarIcon::Pin),
        OverlayAction::Confirm => Some(ToolbarIcon::Confirm),
        OverlayAction::Cancel => Some(ToolbarIcon::Cancel),
        OverlayAction::Tool(Tool::Select) => Some(ToolbarIcon::Select),
        OverlayAction::Option(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OverlayLayout, OverlayOption};

    #[test]
    fn every_main_action_uses_an_original_svg_asset() {
        for icon in ToolbarIcon::ALL {
            let source = icon.source();
            assert!(
                source.starts_with(b"<svg") || source.starts_with(b"<?xml"),
                "{} is not an embedded SVG",
                icon.asset_name()
            );
            let pixels = rasterize_svg(source).expect("original SVG should rasterize");
            assert!(pixels.iter().any(|pixel| pixel.a > 0));
        }
    }

    #[test]
    fn action_to_icon_mapping_names_the_exact_original_asset() {
        let expected = [
            (
                OverlayAction::Tool(Tool::Rectangle),
                ToolbarIcon::Rectangle,
                "square.svg",
            ),
            (
                OverlayAction::Tool(Tool::Circle),
                ToolbarIcon::Circle,
                "circle.svg",
            ),
            (
                OverlayAction::Tool(Tool::Emotion),
                ToolbarIcon::Emotion,
                "emotion.svg",
            ),
            (
                OverlayAction::Tool(Tool::Arrow),
                ToolbarIcon::Arrow,
                "move-up-right.svg",
            ),
            (OverlayAction::Tool(Tool::Pen), ToolbarIcon::Pen, "pen.svg"),
            (
                OverlayAction::Tool(Tool::Mosaic),
                ToolbarIcon::Mosaic,
                "mosaic.svg",
            ),
            (
                OverlayAction::Tool(Tool::Text),
                ToolbarIcon::Text,
                "type.svg",
            ),
            (OverlayAction::Undo, ToolbarIcon::Undo, "undo-2.svg"),
            (
                OverlayAction::ExtractText,
                ToolbarIcon::ExtractText,
                "extracttext.svg",
            ),
            (
                OverlayAction::ScrollCapture,
                ToolbarIcon::ScrollCapture,
                "scroll-capture.svg",
            ),
            (
                OverlayAction::Languages,
                ToolbarIcon::Languages,
                "languages.svg",
            ),
            (OverlayAction::Save, ToolbarIcon::Save, "download.svg"),
            (OverlayAction::Pin, ToolbarIcon::Pin, "pin.svg"),
            (OverlayAction::Confirm, ToolbarIcon::Confirm, "check.svg"),
            (OverlayAction::Cancel, ToolbarIcon::Cancel, "x.svg"),
            (
                OverlayAction::Tool(Tool::Select),
                ToolbarIcon::Select,
                "edit.svg",
            ),
        ];

        for (action, icon, asset_name) in expected {
            assert_eq!(
                icon_for_action(action),
                Some(icon),
                "wrong icon for {action:?}"
            );
            assert_eq!(icon.asset_name(), asset_name, "wrong asset for {action:?}");
        }
        assert_eq!(
            icon_for_action(OverlayAction::Option(OverlayOption::ToggleFill)),
            None
        );
    }

    #[test]
    fn every_capture_toolbar_action_has_exactly_one_icon() {
        let layout = OverlayLayout::for_tool(
            Rect::new(100.0, 100.0, 600.0, 400.0),
            Rect::new(0.0, 0.0, 800.0, 600.0),
            Tool::Select,
        );
        let mapped: Vec<_> = layout
            .buttons
            .iter()
            .map(|button| {
                icon_for_action(button.action).expect("capture action is missing an icon")
            })
            .collect();
        assert_eq!(mapped.len(), layout.buttons.len());
        for (index, icon) in mapped.iter().enumerate() {
            assert!(
                !mapped[..index].contains(icon),
                "capture actions share the {icon:?} icon"
            );
        }
    }

    #[test]
    fn uploaded_texture_slots_follow_the_icon_discriminants() {
        for (slot, icon) in ToolbarIcon::ALL.into_iter().enumerate() {
            assert_eq!(icon as usize, slot, "wrong upload slot for {icon:?}");
        }
    }

    #[test]
    fn normal_toolbar_buttons_use_the_original_18_pixel_glyph_size() {
        let button = Rect::new(10.0, 20.0, 40.0, 50.0);
        let icon = icon_bounds(button);
        assert_eq!(icon, Rect::new(16.0, 26.0, 34.0, 44.0));
        assert_eq!(ICON_IMAGE_FLAGS, ImageFlags::GENERATE_MIPMAPS);
    }
}
