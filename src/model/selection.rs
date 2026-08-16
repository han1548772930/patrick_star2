use super::{Handle, Point, Rect};

const HANDLE_RADIUS: f32 = 7.0;
const MIN_SIZE: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Creating,
    Moving,
    Resizing,
    Ready,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    start: Point,
    original: Option<Rect>,
    handle: Option<Handle>,
}

#[derive(Debug)]
pub struct Selection {
    bounds: Rect,
    rect: Option<Rect>,
    drag: Option<Drag>,
}

impl Selection {
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds: bounds.normalized(),
            rect: None,
            drag: None,
        }
    }

    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    pub fn phase(&self) -> Phase {
        match self.drag {
            Some(Drag { original: None, .. }) => Phase::Creating,
            Some(Drag {
                handle: Some(Handle::Move),
                ..
            }) => Phase::Moving,
            Some(_) => Phase::Resizing,
            None if self.rect.is_some() => Phase::Ready,
            None => Phase::Idle,
        }
    }

    pub fn drag_handle(&self) -> Option<Handle> {
        self.drag
            .and_then(|drag| match (drag.original, drag.handle) {
                (None, _) => None,
                (Some(_), Some(handle)) => Some(handle),
                _ => None,
            })
    }

    pub fn press(&mut self, point: Point) {
        let point = point.clamped(self.bounds);
        let hit = self.hit_test(point);
        match (self.rect, hit) {
            (Some(rect), Some(handle)) => {
                self.drag = Some(Drag {
                    start: point,
                    original: Some(rect),
                    handle: Some(handle),
                });
            }
            _ => {
                self.rect = Some(Rect::new(point.x, point.y, point.x, point.y));
                self.drag = Some(Drag {
                    start: point,
                    original: None,
                    handle: None,
                });
            }
        }
    }

    pub fn drag(&mut self, point: Point) {
        let Some(drag) = self.drag else {
            return;
        };
        let point = point.clamped(self.bounds);
        self.rect = match (drag.original, drag.handle) {
            (None, _) => Some(Rect::from_points(drag.start, point)),
            (Some(original), Some(Handle::Move)) => Some(clamp_move(
                original,
                point.x - drag.start.x,
                point.y - drag.start.y,
                self.bounds,
            )),
            (Some(original), Some(handle)) => Some(resize(original, handle, point, self.bounds)),
            _ => self.rect,
        };
    }

    pub fn release(&mut self) -> bool {
        let changed = self.drag.is_some();
        if let Some(drag) = self.drag.take()
            && let Some(rect) = self.rect
            && (rect.width() < MIN_SIZE || rect.height() < MIN_SIZE)
        {
            self.rect = drag.original;
        }
        changed
    }

    pub fn clear(&mut self) {
        self.rect = None;
        self.drag = None;
    }

    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds.normalized();
        if let Some(rect) = self.rect {
            self.set_rect(rect);
        }
    }

    pub fn set_rect(&mut self, rect: Rect) {
        let rect = rect.normalized().clamped(self.bounds);
        self.rect = (rect.width() >= MIN_SIZE && rect.height() >= MIN_SIZE).then_some(rect);
        self.drag = None;
    }

    pub fn select_all(&mut self) {
        self.rect = Some(self.bounds);
        self.drag = None;
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) -> bool {
        if let Some(rect) = self.rect {
            let next = clamp_move(rect, dx, dy, self.bounds);
            self.rect = Some(next);
            return next != rect;
        }
        false
    }

    pub fn hit_test(&self, point: Point) -> Option<Handle> {
        let rect = self.rect?;
        for (handle, anchor) in handle_points(rect) {
            if (point.x - anchor.x).abs() <= HANDLE_RADIUS
                && (point.y - anchor.y).abs() <= HANDLE_RADIUS
            {
                return Some(handle);
            }
        }
        rect.contains(point).then_some(Handle::Move)
    }
}

pub fn handle_points(rect: Rect) -> [(Handle, Point); 8] {
    Handle::BOX.map(|handle| (handle, handle.position(rect)))
}

fn clamp_move(rect: Rect, dx: f32, dy: f32, bounds: Rect) -> Rect {
    let max_dx = bounds.right - rect.right;
    let min_dx = bounds.left - rect.left;
    let max_dy = bounds.bottom - rect.bottom;
    let min_dy = bounds.top - rect.top;
    rect.translated(dx.clamp(min_dx, max_dx), dy.clamp(min_dy, max_dy))
}

fn resize(original: Rect, handle: Handle, point: Point, bounds: Rect) -> Rect {
    let mut rect = original;
    match handle {
        Handle::TopLeft => {
            rect.left = point.x;
            rect.top = point.y;
        }
        Handle::Top => rect.top = point.y,
        Handle::TopRight => {
            rect.right = point.x;
            rect.top = point.y;
        }
        Handle::Right => rect.right = point.x,
        Handle::BottomRight => {
            rect.right = point.x;
            rect.bottom = point.y;
        }
        Handle::Bottom => rect.bottom = point.y,
        Handle::BottomLeft => {
            rect.left = point.x;
            rect.bottom = point.y;
        }
        Handle::Left => rect.left = point.x,
        Handle::Move | Handle::Start | Handle::End => return original,
    }
    rect.normalized().clamped(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> Selection {
        Selection::new(Rect::new(0.0, 0.0, 800.0, 600.0))
    }

    fn create(selection: &mut Selection, from: Point, to: Point) {
        selection.press(from);
        selection.drag(to);
        selection.release();
    }

    #[test]
    fn creates_a_normalized_region() {
        let mut selection = selection();
        create(
            &mut selection,
            Point::new(300.0, 200.0),
            Point::new(100.0, 50.0),
        );
        assert_eq!(selection.rect(), Some(Rect::new(100.0, 50.0, 300.0, 200.0)));
        assert_eq!(selection.phase(), Phase::Ready);
    }

    #[test]
    fn rejects_a_tiny_new_region() {
        let mut selection = selection();
        create(
            &mut selection,
            Point::new(10.0, 10.0),
            Point::new(12.0, 12.0),
        );
        assert_eq!(selection.rect(), None);
    }

    #[test]
    fn moving_preserves_size_and_stays_inside_bounds() {
        let mut selection = selection();
        create(
            &mut selection,
            Point::new(100.0, 100.0),
            Point::new(300.0, 250.0),
        );
        selection.press(Point::new(200.0, 175.0));
        selection.drag(Point::new(900.0, 900.0));
        selection.release();
        assert_eq!(
            selection.rect(),
            Some(Rect::new(600.0, 450.0, 800.0, 600.0))
        );
    }

    #[test]
    fn resizing_from_a_handle_changes_the_expected_edges() {
        let mut selection = selection();
        create(
            &mut selection,
            Point::new(100.0, 100.0),
            Point::new(300.0, 250.0),
        );
        selection.press(Point::new(100.0, 100.0));
        selection.drag(Point::new(50.0, 75.0));
        selection.release();
        assert_eq!(selection.rect(), Some(Rect::new(50.0, 75.0, 300.0, 250.0)));
    }

    #[test]
    fn invalid_resize_restores_original() {
        let mut selection = selection();
        create(
            &mut selection,
            Point::new(100.0, 100.0),
            Point::new(300.0, 250.0),
        );
        let original = selection.rect();
        selection.press(Point::new(100.0, 100.0));
        selection.drag(Point::new(298.0, 248.0));
        selection.release();
        assert_eq!(selection.rect(), original);
    }

    #[test]
    fn select_all_and_nudge_obey_bounds() {
        let mut selection = selection();
        selection.select_all();
        selection.nudge(100.0, 100.0);
        assert_eq!(selection.rect(), Some(Rect::new(0.0, 0.0, 800.0, 600.0)));
    }
}
