#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Rectangle,
    Circle,
    Emotion,
    Arrow,
    Pen,
    Mosaic,
    Text,
}
