use super::{ANNOTATION_COLORS, Handle, Point, Rect, Rgba};

const HIT_TOLERANCE: f32 = 4.0;
const MIN_TEXT_WIDTH: f32 = 40.0;
const MIN_TEXT_HEIGHT: f32 = 20.0;
const TEXT_LINE_HEIGHT: f32 = 1.2;
const MIN_EMOTION_SIZE: f32 = 16.0;
const MAX_EMOTION_SIZE: f32 = 256.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnnotationId(u64);

impl AnnotationId {
    pub(crate) const fn draft() -> Self {
        Self(u64::MAX)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub color: Rgba,
    pub width: f32,
    pub fill: Option<Rgba>,
}

impl Stroke {
    pub const WIDTHS: [f32; 4] = [2.0, 4.0, 6.0, 10.0];

    pub const fn new(color: Rgba, width: f32) -> Self {
        Self {
            color,
            width,
            fill: None,
        }
    }
}

impl Default for Stroke {
    fn default() -> Self {
        Self::new(ANNOTATION_COLORS[0], Self::WIDTHS[1])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub color: Rgba,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "Microsoft YaHei UI".to_owned(),
            size: 20.0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            color: Rgba::BLACK,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationKind {
    Rectangle {
        rect: Rect,
    },
    Circle {
        rect: Rect,
    },
    Arrow {
        from: Point,
        to: Point,
    },
    Pen {
        points: Vec<Point>,
    },
    Mosaic {
        points: Vec<Point>,
        block_size: u32,
    },
    Text {
        origin: Point,
        content: String,
        style: TextStyle,
    },
    Emotion {
        center: Point,
        glyph: String,
        size: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub id: AnnotationId,
    pub kind: AnnotationKind,
    pub stroke: Stroke,
}

impl Annotation {
    pub fn bounds(&self) -> Rect {
        match &self.kind {
            AnnotationKind::Rectangle { rect } | AnnotationKind::Circle { rect } => {
                rect.normalized()
            }
            AnnotationKind::Arrow { from, to } => Rect::from_points(*from, *to),
            AnnotationKind::Pen { points } | AnnotationKind::Mosaic { points, .. } => {
                point_bounds(points).unwrap_or_default()
            }
            AnnotationKind::Text {
                origin,
                content,
                style,
            } => {
                let lines = content.split('\n');
                let mut line_count = 0usize;
                let mut widest = 0usize;
                for line in lines {
                    line_count += 1;
                    widest = widest.max(line.chars().count());
                }
                Rect::new(
                    origin.x,
                    origin.y,
                    origin.x + (widest as f32 * style.size * 0.6).max(MIN_TEXT_WIDTH),
                    origin.y
                        + (line_count as f32 * style.size * TEXT_LINE_HEIGHT).max(MIN_TEXT_HEIGHT),
                )
            }
            AnnotationKind::Emotion { center, size, .. } => Rect::new(
                center.x - size * 0.5,
                center.y - size * 0.5,
                center.x + size * 0.5,
                center.y + size * 0.5,
            ),
        }
    }

    pub fn visual_bounds(&self) -> Rect {
        match self.kind {
            // Text and emotion glyphs do not use the annotation stroke.
            // Inflating by its width makes their selection frame disagree
            // with the editor's grips, especially while text is being typed.
            AnnotationKind::Text { .. } | AnnotationKind::Emotion { .. } => self.bounds(),
            _ => self.bounds().inflated(self.stroke.width * 0.5 + 1.0),
        }
    }

    pub fn hit_test(&self, point: Point) -> bool {
        let slack = self.stroke.width * 0.5 + HIT_TOLERANCE;
        match &self.kind {
            AnnotationKind::Rectangle { rect } => {
                let rect = rect.normalized();
                (self.stroke.fill.is_some() && rect.contains(point))
                    || (rect.inflated(slack).contains(point)
                        && !rect.inflated(-slack).contains(point))
            }
            AnnotationKind::Circle { rect } => {
                let rect = rect.normalized();
                let center = rect.center();
                let rx = rect.width() * 0.5;
                let ry = rect.height() * 0.5;
                if rx <= 0.0 || ry <= 0.0 {
                    return false;
                }
                let distance = |x_radius: f32, y_radius: f32| {
                    let x = (point.x - center.x) / x_radius.max(0.01);
                    let y = (point.y - center.y) / y_radius.max(0.01);
                    x * x + y * y
                };
                if self.stroke.fill.is_some() && distance(rx, ry) <= 1.0 {
                    return true;
                }
                distance(rx + slack, ry + slack) <= 1.0 && distance(rx - slack, ry - slack) >= 1.0
            }
            AnnotationKind::Arrow { from, to } => distance_to_segment(point, *from, *to) <= slack,
            AnnotationKind::Pen { points } => points
                .windows(2)
                .any(|pair| distance_to_segment(point, pair[0], pair[1]) <= slack),
            AnnotationKind::Mosaic { .. } => false,
            AnnotationKind::Text { .. } | AnnotationKind::Emotion { .. } => {
                self.bounds().inflated(slack).contains(point)
            }
        }
    }

    pub fn movable_hit(&self, point: Point) -> bool {
        match &self.kind {
            AnnotationKind::Rectangle { .. }
            | AnnotationKind::Circle { .. }
            | AnnotationKind::Text { .. }
            | AnnotationKind::Emotion { .. } => self.bounds().contains(point),
            AnnotationKind::Arrow { .. } => self.hit_test(point),
            AnnotationKind::Pen { .. } | AnnotationKind::Mosaic { .. } => false,
        }
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        match &mut self.kind {
            AnnotationKind::Rectangle { rect } | AnnotationKind::Circle { rect } => {
                *rect = rect.translated(dx, dy);
            }
            AnnotationKind::Arrow { from, to } => {
                *from = from.translated(dx, dy);
                *to = to.translated(dx, dy);
            }
            AnnotationKind::Pen { points } | AnnotationKind::Mosaic { points, .. } => {
                for point in points {
                    *point = point.translated(dx, dy);
                }
            }
            AnnotationKind::Text { origin, .. } => {
                *origin = origin.translated(dx, dy);
            }
            AnnotationKind::Emotion { center, .. } => {
                *center = center.translated(dx, dy);
            }
        }
    }

    pub fn resize(&mut self, handle: Handle, point: Point) {
        let before_bounds = self.bounds();
        match &mut self.kind {
            AnnotationKind::Rectangle { rect } | AnnotationKind::Circle { rect } => {
                *rect = resize_rect(*rect, handle, point);
            }
            AnnotationKind::Arrow { from, to } => match handle {
                Handle::Start => *from = point,
                Handle::End => *to = point,
                _ => {}
            },
            AnnotationKind::Text { origin, style, .. } => {
                let after = resize_rect(before_bounds, handle, point);
                let scale = (after.width() / before_bounds.width().max(1.0)).max(0.4);
                *origin = Point::new(after.left, after.top);
                style.size = (style.size * scale).clamp(8.0, 144.0);
            }
            AnnotationKind::Emotion { center, size, .. } => {
                let after = resize_square(before_bounds, handle, point);
                *center = after.center();
                *size = after.width();
            }
            AnnotationKind::Pen { .. } | AnnotationKind::Mosaic { .. } => {}
        }
    }

    pub fn handles(&self) -> &'static [Handle] {
        match &self.kind {
            AnnotationKind::Rectangle { .. } | AnnotationKind::Circle { .. } => &Handle::BOX,
            AnnotationKind::Arrow { .. } => &[Handle::Start, Handle::End],
            AnnotationKind::Text { .. } | AnnotationKind::Emotion { .. } => &Handle::CORNERS,
            AnnotationKind::Pen { .. } | AnnotationKind::Mosaic { .. } => &[],
        }
    }

    pub fn handle_position(&self, handle: Handle) -> Point {
        match (&self.kind, handle) {
            (AnnotationKind::Arrow { from, .. }, Handle::Start) => *from,
            (AnnotationKind::Arrow { to, .. }, Handle::End) => *to,
            _ => handle.position(self.bounds()),
        }
    }

    pub fn handle_at(&self, point: Point, radius: f32) -> Option<Handle> {
        self.handles().iter().copied().find(|handle| {
            let position = self.handle_position(*handle);
            position.distance(point) <= radius
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnnotationDocument {
    items: Vec<Annotation>,
    next_id: u64,
}

impl AnnotationDocument {
    pub fn items(&self) -> &[Annotation] {
        &self.items
    }

    pub fn add(&mut self, kind: AnnotationKind, stroke: Stroke) -> AnnotationId {
        let id = AnnotationId(self.next_id);
        self.next_id += 1;
        self.items.push(Annotation { id, kind, stroke });
        id
    }

    pub fn get(&self, id: AnnotationId) -> Option<&Annotation> {
        self.items.iter().find(|annotation| annotation.id == id)
    }

    pub fn get_mut(&mut self, id: AnnotationId) -> Option<&mut Annotation> {
        self.items.iter_mut().find(|annotation| annotation.id == id)
    }

    pub fn remove(&mut self, id: AnnotationId) -> bool {
        let Some(index) = self.items.iter().position(|annotation| annotation.id == id) else {
            return false;
        };
        self.items.remove(index);
        true
    }

    pub fn hit_test(&self, point: Point) -> Option<AnnotationId> {
        self.items
            .iter()
            .rev()
            .find(|annotation| annotation.hit_test(point))
            .map(|annotation| annotation.id)
    }
}

fn point_bounds(points: &[Point]) -> Option<Rect> {
    points.iter().copied().fold(None, |bounds, point| {
        let point = Rect::new(point.x, point.y, point.x, point.y);
        Some(bounds.map_or(point, |bounds: Rect| bounds.union(point)))
    })
}

fn distance_to_segment(point: Point, start: Point, end: Point) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let position =
        (((point.x - start.x) * dx + (point.y - start.y) * dy) / length_squared).clamp(0.0, 1.0);
    point.distance(Point::new(start.x + dx * position, start.y + dy * position))
}

fn resize_rect(rect: Rect, handle: Handle, point: Point) -> Rect {
    let mut next = rect;
    match handle {
        Handle::TopLeft => {
            next.left = point.x;
            next.top = point.y;
        }
        Handle::Top => next.top = point.y,
        Handle::TopRight => {
            next.right = point.x;
            next.top = point.y;
        }
        Handle::Right => next.right = point.x,
        Handle::BottomRight => {
            next.right = point.x;
            next.bottom = point.y;
        }
        Handle::Bottom => next.bottom = point.y,
        Handle::BottomLeft => {
            next.left = point.x;
            next.bottom = point.y;
        }
        Handle::Left => next.left = point.x,
        Handle::Move | Handle::Start | Handle::End => return rect,
    }
    next.normalized()
}

fn resize_square(rect: Rect, handle: Handle, point: Point) -> Rect {
    let rect = rect.normalized();
    let (anchor, x_direction, y_direction) = match handle {
        Handle::TopLeft => (Point::new(rect.right, rect.bottom), -1.0, -1.0),
        Handle::TopRight => (Point::new(rect.left, rect.bottom), 1.0, -1.0),
        Handle::BottomRight => (Point::new(rect.left, rect.top), 1.0, 1.0),
        Handle::BottomLeft => (Point::new(rect.right, rect.top), -1.0, 1.0),
        _ => return rect,
    };
    let horizontal = (point.x - anchor.x) * x_direction;
    let vertical = (point.y - anchor.y) * y_direction;
    let size = ((horizontal + vertical) * 0.5).clamp(MIN_EMOTION_SIZE, MAX_EMOTION_SIZE);
    Rect::from_points(
        anchor,
        Point::new(anchor.x + size * x_direction, anchor.y + size * y_direction),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(rect: Rect) -> Annotation {
        Annotation {
            id: AnnotationId(0),
            kind: AnnotationKind::Rectangle { rect },
            stroke: Stroke::default(),
        }
    }

    #[test]
    fn outlined_rectangle_does_not_steal_interior_clicks() {
        let annotation = rectangle(Rect::new(10.0, 10.0, 110.0, 60.0));
        assert!(annotation.hit_test(Point::new(10.0, 30.0)));
        assert!(!annotation.hit_test(Point::new(60.0, 35.0)));
    }

    #[test]
    fn document_hits_topmost_annotation() {
        let mut document = AnnotationDocument::default();
        let bottom = document.add(
            AnnotationKind::Rectangle {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            Stroke::default(),
        );
        let top = document.add(
            AnnotationKind::Rectangle {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            Stroke::default(),
        );
        assert_ne!(bottom, top);
        assert_eq!(document.hit_test(Point::new(0.0, 50.0)), Some(top));
    }

    #[test]
    fn text_visual_frame_is_not_inflated_by_shape_stroke() {
        let annotation = Annotation {
            id: AnnotationId(0),
            kind: AnnotationKind::Text {
                origin: Point::new(20.0, 30.0),
                content: "text".to_owned(),
                style: TextStyle::default(),
            },
            stroke: Stroke::new(Rgba::BLACK, 10.0),
        };

        assert_eq!(annotation.visual_bounds(), annotation.bounds());
    }

    #[test]
    fn arrow_exposes_only_its_real_endpoint_handles() {
        let annotation = Annotation {
            id: AnnotationId(0),
            kind: AnnotationKind::Arrow {
                from: Point::new(80.0, 10.0),
                to: Point::new(20.0, 90.0),
            },
            stroke: Stroke::default(),
        };
        assert_eq!(annotation.handles(), &[Handle::Start, Handle::End]);
        assert_eq!(
            annotation.handle_position(Handle::Start),
            Point::new(80.0, 10.0)
        );
        assert_eq!(
            annotation.handle_position(Handle::End),
            Point::new(20.0, 90.0)
        );
    }

    #[test]
    fn mosaic_is_retained_but_never_selectable() {
        let annotation = Annotation {
            id: AnnotationId(0),
            kind: AnnotationKind::Mosaic {
                points: vec![Point::new(0.0, 0.0), Point::new(100.0, 100.0)],
                block_size: 16,
            },
            stroke: Stroke::default(),
        };
        assert!(!annotation.hit_test(Point::new(50.0, 50.0)));
    }

    #[test]
    fn emotion_corner_resize_is_square_and_keeps_the_opposite_corner() {
        let cases = [
            (
                Handle::TopLeft,
                Point::new(56.0, 36.0),
                Point::new(156.0, 136.0),
            ),
            (
                Handle::TopRight,
                Point::new(200.0, 36.0),
                Point::new(100.0, 136.0),
            ),
            (
                Handle::BottomRight,
                Point::new(200.0, 180.0),
                Point::new(100.0, 80.0),
            ),
            (
                Handle::BottomLeft,
                Point::new(56.0, 180.0),
                Point::new(156.0, 80.0),
            ),
        ];

        for (handle, pointer, fixed_corner) in cases {
            let stroke = Stroke::new(Rgba::rgb(18, 52, 86), 10.0);
            let mut annotation = Annotation {
                id: AnnotationId(0),
                kind: AnnotationKind::Emotion {
                    center: Point::new(128.0, 108.0),
                    glyph: "\u{1f600}".to_owned(),
                    size: 56.0,
                },
                stroke,
            };

            annotation.resize(handle, pointer);

            let bounds = annotation.bounds();
            assert_eq!(bounds.width(), 100.0, "{handle:?}");
            assert_eq!(bounds.height(), 100.0, "{handle:?}");
            assert_eq!(handle.position(bounds), pointer, "{handle:?}");
            assert_eq!(
                opposite_corner(handle).position(bounds),
                fixed_corner,
                "{handle:?}"
            );
            assert_eq!(annotation.visual_bounds(), bounds, "{handle:?}");
            assert_eq!(annotation.stroke, stroke, "{handle:?}");
            let AnnotationKind::Emotion { glyph, .. } = &annotation.kind else {
                panic!("expected emotion annotation");
            };
            assert_eq!(glyph, "\u{1f600}", "{handle:?}");
        }
    }

    fn opposite_corner(handle: Handle) -> Handle {
        match handle {
            Handle::TopLeft => Handle::BottomRight,
            Handle::TopRight => Handle::BottomLeft,
            Handle::BottomRight => Handle::TopLeft,
            Handle::BottomLeft => Handle::TopRight,
            _ => panic!("expected corner handle"),
        }
    }
}
