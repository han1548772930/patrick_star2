use super::{Editor, EditorKey, Point, Rect, RgbaFrame, Tool};

const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 32.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QuarterTurn {
    #[default]
    Zero,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

impl QuarterTurn {
    pub const fn clockwise(self) -> Self {
        match self {
            Self::Zero => Self::Clockwise90,
            Self::Clockwise90 => Self::Clockwise180,
            Self::Clockwise180 => Self::Clockwise270,
            Self::Clockwise270 => Self::Zero,
        }
    }

    pub const fn swaps_axes(self) -> bool {
        matches!(self, Self::Clockwise90 | Self::Clockwise270)
    }

    pub const fn angle_radians(self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::Clockwise90 => core::f32::consts::FRAC_PI_2,
            Self::Clockwise180 => core::f32::consts::PI,
            Self::Clockwise270 => core::f32::consts::PI + core::f32::consts::FRAC_PI_2,
        }
    }

    fn rotate_vector(self, vector: Point) -> Point {
        match self {
            Self::Zero => vector,
            Self::Clockwise90 => Point::new(-vector.y, vector.x),
            Self::Clockwise180 => Point::new(-vector.x, -vector.y),
            Self::Clockwise270 => Point::new(vector.y, -vector.x),
        }
    }

    fn inverse_rotate_vector(self, vector: Point) -> Point {
        match self {
            Self::Zero => vector,
            Self::Clockwise90 => Point::new(vector.y, -vector.x),
            Self::Clockwise180 => Point::new(-vector.x, -vector.y),
            Self::Clockwise270 => Point::new(-vector.y, vector.x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    document_width: f32,
    document_height: f32,
    canvas_width: f32,
    canvas_height: f32,
    zoom: f32,
    pan: Point,
    rotation: QuarterTurn,
    fitted: bool,
}

impl ViewTransform {
    pub fn new(document_width: u32, document_height: u32) -> Self {
        Self {
            document_width: document_width as f32,
            document_height: document_height as f32,
            canvas_width: 1.0,
            canvas_height: 1.0,
            zoom: 1.0,
            pan: Point::new(0.0, 0.0),
            rotation: QuarterTurn::Zero,
            fitted: true,
        }
    }

    pub fn set_canvas_size(&mut self, width: f32, height: f32) -> bool {
        let width = width.max(1.0);
        let height = height.max(1.0);
        if self.canvas_width == width && self.canvas_height == height {
            return false;
        }
        self.canvas_width = width;
        self.canvas_height = height;
        if self.fitted {
            self.apply_fit();
        }
        true
    }

    pub fn fit_to_canvas(&mut self) -> bool {
        let before = (self.zoom, self.pan, self.fitted);
        self.fitted = true;
        self.apply_fit();
        before != (self.zoom, self.pan, self.fitted)
    }

    pub fn actual_size(&mut self) -> bool {
        let before = (self.zoom, self.pan, self.fitted);
        self.zoom = 1.0;
        self.pan = Point::new(0.0, 0.0);
        self.fitted = false;
        before != (self.zoom, self.pan, self.fitted)
    }

    pub fn zoom_at(&mut self, canvas_point: Point, factor: f32) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let document_anchor = self.canvas_to_document(canvas_point);
        let next = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (next - self.zoom).abs() < f32::EPSILON {
            return false;
        }
        self.zoom = next;
        self.fitted = false;
        let rotated = self
            .rotation
            .rotate_vector(document_anchor - self.document_center());
        self.pan = canvas_point - self.canvas_center() - rotated * self.zoom;
        true
    }

    pub fn pan_by(&mut self, delta: Point) -> bool {
        if !delta.x.is_finite()
            || !delta.y.is_finite()
            || (delta.x.abs() < f32::EPSILON && delta.y.abs() < f32::EPSILON)
        {
            return false;
        }
        self.pan = self.pan + delta;
        self.fitted = false;
        true
    }

    pub fn rotate_clockwise(&mut self) -> bool {
        self.rotation = self.rotation.clockwise();
        self.fitted = true;
        self.apply_fit();
        true
    }

    pub fn document_to_canvas(self, point: Point) -> Point {
        let rotated = self.rotation.rotate_vector(point - self.document_center());
        self.canvas_center() + self.pan + rotated * self.zoom
    }

    pub fn canvas_to_document(self, point: Point) -> Point {
        let rotated = (point - self.canvas_center() - self.pan) * (1.0 / self.zoom);
        self.document_center() + self.rotation.inverse_rotate_vector(rotated)
    }

    pub fn document_bounds(self) -> Rect {
        Rect::new(0.0, 0.0, self.document_width, self.document_height)
    }

    pub fn canvas_bounds(self) -> Rect {
        Rect::new(0.0, 0.0, self.canvas_width, self.canvas_height)
    }

    pub const fn zoom(self) -> f32 {
        self.zoom
    }

    pub const fn pan(self) -> Point {
        self.pan
    }

    pub const fn rotation(self) -> QuarterTurn {
        self.rotation
    }

    pub fn displayed_bounds(self) -> Rect {
        let corners = [
            self.document_to_canvas(Point::new(0.0, 0.0)),
            self.document_to_canvas(Point::new(self.document_width, 0.0)),
            self.document_to_canvas(Point::new(0.0, self.document_height)),
            self.document_to_canvas(Point::new(self.document_width, self.document_height)),
        ];
        let mut left = f32::INFINITY;
        let mut top = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        for point in corners {
            left = left.min(point.x);
            top = top.min(point.y);
            right = right.max(point.x);
            bottom = bottom.max(point.y);
        }
        Rect::new(left, top, right, bottom)
    }

    fn apply_fit(&mut self) {
        let (width, height) = if self.rotation.swaps_axes() {
            (self.document_height, self.document_width)
        } else {
            (self.document_width, self.document_height)
        };
        self.zoom = (self.canvas_width / width)
            .min(self.canvas_height / height)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = Point::new(0.0, 0.0);
    }

    fn document_center(self) -> Point {
        Point::new(self.document_width * 0.5, self.document_height * 0.5)
    }

    fn canvas_center(self) -> Point {
        Point::new(self.canvas_width * 0.5, self.canvas_height * 0.5)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PreviewMode {
    #[default]
    Edit,
    Pan,
}

#[derive(Debug)]
pub struct PreviewSession {
    image: RgbaFrame,
    editor: Editor,
    view: ViewTransform,
    mode: PreviewMode,
    pan_anchor: Option<Point>,
}

impl PreviewSession {
    pub fn new(image: RgbaFrame) -> Self {
        let view = ViewTransform::new(image.width(), image.height());
        Self {
            image,
            editor: Editor::new(),
            view,
            mode: PreviewMode::Edit,
            pan_anchor: None,
        }
    }

    pub fn image(&self) -> &RgbaFrame {
        &self.image
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn view(&self) -> ViewTransform {
        self.view
    }

    pub fn mode(&self) -> PreviewMode {
        self.mode
    }

    pub fn set_canvas_size(&mut self, width: f32, height: f32) -> bool {
        self.view.set_canvas_size(width, height)
    }

    pub fn set_tool(&mut self, tool: Tool) -> bool {
        let changed = self.mode != PreviewMode::Edit || self.editor.tool() != tool;
        self.mode = PreviewMode::Edit;
        self.pan_anchor = None;
        self.editor.set_tool(tool);
        changed
    }

    pub fn set_pan_mode(&mut self) -> bool {
        if self.mode == PreviewMode::Pan {
            return false;
        }
        self.mode = PreviewMode::Pan;
        self.pan_anchor = None;
        true
    }

    pub fn pointer_down(&mut self, point: Point) -> bool {
        match self.mode {
            PreviewMode::Pan => {
                self.pan_anchor = Some(point);
                true
            }
            PreviewMode::Edit => {
                let document = self.view.canvas_to_document(point);
                self.editor.press(document, self.view.document_bounds())
            }
        }
    }

    pub fn pointer_move(&mut self, point: Point) -> bool {
        match self.mode {
            PreviewMode::Pan => {
                let Some(previous) = self.pan_anchor.replace(point) else {
                    return false;
                };
                self.view.pan_by(point - previous)
            }
            PreviewMode::Edit => {
                let document = self.view.canvas_to_document(point);
                self.editor
                    .pointer_move(document, self.view.document_bounds())
            }
        }
    }

    pub fn double_click(&mut self, point: Point) -> bool {
        if self.mode != PreviewMode::Edit {
            return false;
        }
        self.editor
            .double_click(self.view.canvas_to_document(point))
    }

    pub fn pointer_up(&mut self) -> bool {
        match self.mode {
            PreviewMode::Pan => self.pan_anchor.take().is_some(),
            PreviewMode::Edit => self.editor.release(),
        }
    }

    pub fn zoom_at(&mut self, point: Point, wheel_delta: f32) -> bool {
        let factor = (wheel_delta / 480.0).exp();
        self.view.zoom_at(point, factor)
    }

    pub fn zoom_by(&mut self, factor: f32) -> bool {
        let canvas = self.view.canvas_bounds();
        self.view.zoom_at(
            Point::new(canvas.width() * 0.5, canvas.height() * 0.5),
            factor,
        )
    }

    pub fn fit_to_canvas(&mut self) -> bool {
        self.view.fit_to_canvas()
    }

    pub fn actual_size(&mut self) -> bool {
        self.view.actual_size()
    }

    pub fn rotate_clockwise(&mut self) -> bool {
        self.view.rotate_clockwise()
    }

    pub fn key(&mut self, key: EditorKey) -> bool {
        self.editor.key(key)
    }

    pub fn insert_character(&mut self, character: char) -> bool {
        self.editor.insert_char(character)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RectI;

    fn session(width: u32, height: u32) -> PreviewSession {
        PreviewSession::new(
            RgbaFrame::new(
                RectI::new(-20, 30, width, height),
                vec![0; width as usize * height as usize * 4],
            )
            .unwrap(),
        )
    }

    fn close(left: Point, right: Point) {
        assert!(
            (left.x - right.x).abs() < 0.001,
            "x differs: {left:?} != {right:?}"
        );
        assert!(
            (left.y - right.y).abs() < 0.001,
            "y differs: {left:?} != {right:?}"
        );
    }

    #[test]
    fn fit_centers_a_wide_document_without_upscaling_past_the_canvas() {
        let mut view = ViewTransform::new(1200, 600);
        view.set_canvas_size(800.0, 600.0);
        assert!((view.zoom() - 2.0 / 3.0).abs() < 0.001);
        assert_eq!(view.displayed_bounds(), Rect::new(0.0, 100.0, 800.0, 500.0));
    }

    #[test]
    fn every_quarter_turn_round_trips_canvas_coordinates() {
        let mut view = ViewTransform::new(640, 360);
        view.set_canvas_size(1000.0, 700.0);
        let document = Point::new(127.25, 219.75);
        for _ in 0..4 {
            let canvas = view.document_to_canvas(document);
            close(view.canvas_to_document(canvas), document);
            view.rotate_clockwise();
        }
    }

    #[test]
    fn zoom_keeps_the_document_pixel_under_the_pointer_fixed() {
        let mut view = ViewTransform::new(800, 600);
        view.set_canvas_size(1000.0, 700.0);
        let cursor = Point::new(173.0, 412.0);
        let before = view.canvas_to_document(cursor);
        assert!(view.zoom_at(cursor, 1.8));
        close(view.canvas_to_document(cursor), before);
    }

    #[test]
    fn actual_size_restores_one_document_pixel_per_logical_pixel() {
        let mut view = ViewTransform::new(1600, 900);
        view.set_canvas_size(800.0, 600.0);
        assert!(view.zoom() < 1.0);
        assert!(view.actual_size());
        assert_eq!(view.zoom(), 1.0);
        assert_eq!(view.pan(), Point::new(0.0, 0.0));
    }

    #[test]
    fn pan_mode_does_not_create_an_annotation() {
        let mut preview = session(800, 600);
        preview.set_canvas_size(800.0, 600.0);
        preview.set_pan_mode();
        assert!(preview.pointer_down(Point::new(100.0, 100.0)));
        assert!(preview.pointer_move(Point::new(140.0, 125.0)));
        assert!(preview.pointer_up());
        assert_eq!(preview.editor().annotations().items().len(), 0);
        assert_eq!(preview.view().pan(), Point::new(40.0, 25.0));
    }

    #[test]
    fn edit_input_is_mapped_back_to_document_pixels_after_zoom_and_rotation() {
        let mut preview = session(400, 200);
        preview.set_canvas_size(800.0, 600.0);
        preview.rotate_clockwise();
        preview.set_tool(Tool::Rectangle);
        let start = preview.view().document_to_canvas(Point::new(20.0, 30.0));
        let end = preview.view().document_to_canvas(Point::new(120.0, 90.0));
        assert!(preview.pointer_down(start));
        assert!(preview.pointer_move(end));
        assert!(preview.pointer_up());
        assert_eq!(
            preview.editor().annotations().items()[0].bounds(),
            Rect::new(20.0, 30.0, 120.0, 90.0)
        );
    }

    #[test]
    fn text_double_click_is_mapped_through_the_preview_transform() {
        let mut preview = session(400, 200);
        preview.set_canvas_size(800.0, 600.0);
        preview.set_tool(Tool::Text);
        let document = Point::new(40.0, 50.0);
        let canvas = preview.view().document_to_canvas(document);
        assert!(preview.pointer_down(canvas));
        assert!(preview.insert_character('A'));
        assert!(preview.key(EditorKey::Escape));

        preview.rotate_clockwise();
        let canvas = preview
            .view()
            .document_to_canvas(Point::new(document.x + 3.0, document.y + 3.0));
        assert!(preview.double_click(canvas));
        assert!(preview.editor().caret().is_some());
    }
}
