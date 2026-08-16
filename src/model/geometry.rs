#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn clamped(self, bounds: Rect) -> Self {
        Self::new(
            self.x.clamp(bounds.left, bounds.right),
            self.y.clamp(bounds.top, bounds.bottom),
        )
    }

    pub fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }
}

impl std::ops::Add for Point {
    type Output = Self;

    fn add(self, right: Self) -> Self::Output {
        Self::new(self.x + right.x, self.y + right.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Self;

    fn sub(self, right: Self) -> Self::Output {
        Self::new(self.x - right.x, self.y - right.y)
    }
}

impl std::ops::Mul<f32> for Point {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self::Output {
        Self::new(self.x * scalar, self.y * scalar)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    Move,
    Start,
    End,
}

impl Handle {
    pub const BOX: [Self; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];

    pub const CORNERS: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomRight,
        Self::BottomLeft,
    ];

    pub fn position(self, rect: Rect) -> Point {
        let center = rect.center();
        match self {
            Self::TopLeft | Self::Start => Point::new(rect.left, rect.top),
            Self::Top => Point::new(center.x, rect.top),
            Self::TopRight => Point::new(rect.right, rect.top),
            Self::Right => Point::new(rect.right, center.y),
            Self::BottomRight | Self::End => Point::new(rect.right, rect.bottom),
            Self::Bottom => Point::new(center.x, rect.bottom),
            Self::BottomLeft => Point::new(rect.left, rect.bottom),
            Self::Left => Point::new(rect.left, center.y),
            Self::Move => center,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PointI {
    pub x: i32,
    pub y: i32,
}

impl PointI {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn from_points(a: Point, b: Point) -> Self {
        Self::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
    }

    pub fn width(self) -> f32 {
        (self.right - self.left).max(0.0)
    }

    pub fn height(self) -> f32 {
        (self.bottom - self.top).max(0.0)
    }

    pub fn contains(self, point: Point) -> bool {
        point.x >= self.left
            && point.x <= self.right
            && point.y >= self.top
            && point.y <= self.bottom
    }

    pub fn center(self) -> Point {
        Point::new(
            (self.left + self.right) * 0.5,
            (self.top + self.bottom) * 0.5,
        )
    }

    pub fn inflated(self, amount: f32) -> Self {
        Self::new(
            self.left - amount,
            self.top - amount,
            self.right + amount,
            self.bottom + amount,
        )
    }

    pub fn union(self, other: Self) -> Self {
        Self::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }

    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self::new(
            self.left + dx,
            self.top + dy,
            self.right + dx,
            self.bottom + dy,
        )
    }

    pub fn normalized(self) -> Self {
        Self::new(
            self.left.min(self.right),
            self.top.min(self.bottom),
            self.left.max(self.right),
            self.top.max(self.bottom),
        )
    }

    pub fn clamped(self, bounds: Rect) -> Self {
        let normalized = self.normalized();
        Self::new(
            normalized.left.clamp(bounds.left, bounds.right),
            normalized.top.clamp(bounds.top, bounds.bottom),
            normalized.right.clamp(bounds.left, bounds.right),
            normalized.bottom.clamp(bounds.top, bounds.bottom),
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RectI {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl RectI {
    pub const fn new(left: i32, top: i32, width: u32, height: u32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    pub fn right(self) -> i32 {
        self.left.saturating_add_unsigned(self.width)
    }

    pub fn bottom(self) -> i32 {
        self.top.saturating_add_unsigned(self.height)
    }

    pub fn contains(self, point: PointI) -> bool {
        point.x >= self.left
            && point.x < self.right()
            && point.y >= self.top
            && point.y < self.bottom()
    }

    pub fn intersection(self, other: RectI) -> Option<RectI> {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > left && bottom > top)
            .then(|| RectI::new(left, top, (right - left) as u32, (bottom - top) as u32))
    }

    pub fn local_bounds(self) -> Rect {
        Rect::new(0.0, 0.0, self.width as f32, self.height as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_points_normalizes_coordinates() {
        assert_eq!(
            Rect::from_points(Point::new(8.0, 9.0), Point::new(2.0, 3.0)),
            Rect::new(2.0, 3.0, 8.0, 9.0)
        );
    }

    #[test]
    fn desktop_origin_does_not_leak_into_local_coordinates() {
        let desktop = RectI::new(-1920, -300, 3840, 1380);
        assert_eq!(desktop.local_bounds(), Rect::new(0.0, 0.0, 3840.0, 1380.0));
    }
}
