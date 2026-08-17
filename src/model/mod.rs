mod annotation;
mod capture;
mod color;
mod cursor;
mod editor;
mod export;
mod frame;
mod geometry;
mod history;
mod overlay;
#[allow(dead_code)]
pub(crate) mod preview;
mod selection;
mod tool;

pub use annotation::{
    Annotation, AnnotationDocument, AnnotationId, AnnotationKind, Stroke, TextStyle,
};
pub use capture::{DetectedTarget, OverlayFeatures, OverlaySession, TargetKind};
pub use color::{ANNOTATION_COLORS, Rgba};
pub use cursor::{PointerCursor, capture_cursor, preview_cursor};
pub use editor::{Editor, EditorKey};
pub use export::{CaptureIntent, ExportRegion, RgbaFrame};
pub use frame::DesktopFrame;
pub use geometry::{Handle, Point, PointI, Rect, RectI};
pub use history::History;
pub use overlay::{
    EMOTIONS, MOSAIC_BLOCK_SIZES, OptionsLayout, OverlayAction, OverlayLayout, OverlayOption,
    STROKE_WIDTHS, ScrollAction, ScrollLayout, TEXT_SIZES, TOOLBAR_COLORS,
};
pub use selection::{Phase, Selection, handle_points};
pub use tool::Tool;

#[derive(Debug)]
pub enum CaptureOutcome {
    Cancelled,
    Confirmed {
        image: RgbaFrame,
        intent: CaptureIntent,
        desktop: DesktopFrame,
    },
}
