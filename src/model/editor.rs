use super::{
    Annotation, AnnotationDocument, AnnotationId, AnnotationKind, Handle, History, Point, Rect,
    Stroke, TextStyle, Tool,
};

const DRAG_THRESHOLD: f32 = 3.0;
const MIN_SHAPE: f32 = 5.0;
const GRIP_RADIUS: f32 = 8.0;
const DRAFT_ID: AnnotationId = AnnotationId::draft();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKey {
    Escape,
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Undo,
    Redo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub annotation: AnnotationId,
    pub index: usize,
}

#[derive(Debug, Clone)]
enum Gesture {
    Idle,
    Drawing {
        start: Point,
        draft: Annotation,
    },
    Moving {
        start: Point,
        before: Annotation,
        document_before: AnnotationDocument,
    },
    Resizing {
        start: Point,
        handle: Handle,
        before: Annotation,
        document_before: AnnotationDocument,
    },
}

#[derive(Debug, Clone)]
pub struct Editor {
    document: History<AnnotationDocument>,
    tool: Tool,
    stroke: Stroke,
    text_style: TextStyle,
    mosaic_block_size: u32,
    emotion: String,
    selected: Option<AnnotationId>,
    hovered: Option<AnnotationId>,
    gesture: Gesture,
    caret: Option<Caret>,
    text_baseline: Option<AnnotationDocument>,
    mosaic_generation: u64,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            document: History::new(AnnotationDocument::default()),
            tool: Tool::Select,
            stroke: Stroke::default(),
            text_style: TextStyle::default(),
            mosaic_block_size: 16,
            emotion: "\u{1F642}".to_owned(),
            selected: None,
            hovered: None,
            gesture: Gesture::Idle,
            caret: None,
            text_baseline: None,
            mosaic_generation: 0,
        }
    }

    pub fn annotations(&self) -> &AnnotationDocument {
        self.document.current()
    }

    pub fn draft(&self) -> Option<&Annotation> {
        match &self.gesture {
            Gesture::Drawing { draft, .. } => Some(draft),
            _ => None,
        }
    }

    pub fn selected_annotation(&self) -> Option<&Annotation> {
        self.document.current().get(self.selected?)
    }

    pub fn hovered_annotation(&self) -> Option<AnnotationId> {
        self.hovered
    }

    pub fn caret(&self) -> Option<Caret> {
        self.caret
    }

    pub fn is_editing_text(&self) -> bool {
        self.caret.is_some()
    }

    pub fn drag_handle(&self) -> Option<Handle> {
        match self.gesture {
            Gesture::Moving { .. } => Some(Handle::Move),
            Gesture::Resizing { handle, .. } => Some(handle),
            Gesture::Idle | Gesture::Drawing { .. } => None,
        }
    }

    pub fn grip_under(&self, point: Point) -> Option<Handle> {
        let annotation = self.selected_annotation()?;
        let editable_text = matches!(annotation.kind, AnnotationKind::Text { .. });
        if (!editable_text || self.caret.is_some())
            && let Some(handle) = annotation.handle_at(point, GRIP_RADIUS)
        {
            return Some(handle);
        }
        annotation.movable_hit(point).then_some(Handle::Move)
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn stroke(&self) -> Stroke {
        self.stroke
    }

    pub fn text_style(&self) -> &TextStyle {
        &self.text_style
    }

    pub fn mosaic_block_size(&self) -> u32 {
        self.mosaic_block_size
    }

    pub fn emotion(&self) -> &str {
        &self.emotion
    }

    pub fn can_undo(&self) -> bool {
        self.document.can_undo()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        self.document.can_redo()
    }

    pub fn mosaic_generation(&self) -> u64 {
        self.mosaic_generation
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.stop_text_editing();
        self.tool = tool;
        self.selected = None;
        self.hovered = None;
        self.gesture = Gesture::Idle;
    }

    pub fn set_stroke(&mut self, stroke: Stroke) -> bool {
        let default_changed = self.stroke != stroke;
        self.stroke = stroke;
        let Some(id) = self.selected else {
            return default_changed;
        };
        let applies = self.document.current().get(id).is_some_and(|annotation| {
            !matches!(
                annotation.kind,
                AnnotationKind::Text { .. } | AnnotationKind::Emotion { .. }
            )
        });
        if !applies {
            return default_changed;
        }
        let before = self.document.current().clone();
        if let Some(annotation) = self.document.edit().get_mut(id) {
            annotation.stroke = stroke;
        }
        self.document.commit_from(before) || default_changed
    }

    pub fn set_fill(&mut self, fill: Option<super::Rgba>) -> bool {
        let default_changed = self.stroke.fill != fill;
        self.stroke.fill = fill;
        let Some(id) = self.selected else {
            return default_changed;
        };
        let applies = self.document.current().get(id).is_some_and(|annotation| {
            matches!(
                annotation.kind,
                AnnotationKind::Rectangle { .. } | AnnotationKind::Circle { .. }
            )
        });
        if !applies {
            return default_changed;
        }
        let before = self.document.current().clone();
        if let Some(annotation) = self.document.edit().get_mut(id) {
            annotation.stroke.fill = fill;
        }
        self.document.commit_from(before) || default_changed
    }

    pub fn set_text_style(&mut self, style: TextStyle) -> bool {
        let default_changed = self.text_style != style;
        self.text_style = style.clone();
        let Some(id) = self.caret.map(|caret| caret.annotation).or(self.selected) else {
            return default_changed;
        };
        let applies = self
            .document
            .current()
            .get(id)
            .is_some_and(|annotation| matches!(annotation.kind, AnnotationKind::Text { .. }));
        if !applies {
            return default_changed;
        }
        if self.caret.is_some() {
            if let Some(annotation) = self.document.edit().get_mut(id)
                && let AnnotationKind::Text {
                    style: annotation_style,
                    ..
                } = &mut annotation.kind
            {
                *annotation_style = style;
            }
            return true;
        }
        let before = self.document.current().clone();
        if let Some(annotation) = self.document.edit().get_mut(id)
            && let AnnotationKind::Text {
                style: annotation_style,
                ..
            } = &mut annotation.kind
        {
            *annotation_style = style;
        }
        self.document.commit_from(before) || default_changed
    }

    pub fn set_mosaic_block_size(&mut self, block_size: u32) -> bool {
        let block_size = block_size.clamp(4, 64);
        if self.mosaic_block_size == block_size {
            return false;
        }
        self.mosaic_block_size = block_size;
        true
    }

    pub fn set_emotion(&mut self, emotion: &str) -> bool {
        if self.emotion == emotion {
            return false;
        }
        emotion.clone_into(&mut self.emotion);
        true
    }

    pub fn insert_emotion(&mut self, center: Point, emotion: &str) -> bool {
        self.stop_text_editing();
        self.set_emotion(emotion);
        let before = self.document.current().clone();
        let id = self.document.edit().add(
            AnnotationKind::Emotion {
                center,
                glyph: self.emotion.clone(),
                size: 56.0,
            },
            self.stroke,
        );
        self.document.commit_from(before);
        self.selected = Some(id);
        self.tool = Tool::Emotion;
        true
    }

    pub fn press(&mut self, point: Point, region: Rect) -> bool {
        if let Some(caret) = self.caret {
            if let Some(annotation) = self.document.current().get(caret.annotation) {
                if let Some(handle) = annotation.handle_at(point, GRIP_RADIUS) {
                    self.begin_resize(annotation.clone(), handle, point);
                    return true;
                }
                if annotation.bounds().contains(point) {
                    self.place_caret(point);
                    return true;
                }
            }
            self.stop_text_editing();
            return true;
        }

        if let Some(annotation) = self.selected_annotation().cloned() {
            if let Some(handle) = annotation.handle_at(point, GRIP_RADIUS) {
                self.begin_resize(annotation, handle, point);
                return true;
            }
            if annotation.movable_hit(point) {
                self.begin_move(annotation, point);
                return true;
            }
        }

        if !region.contains(point) {
            return false;
        }

        if let Some(id) = self.document.current().hit_test(point) {
            let Some(annotation) = self.document.current().get(id).cloned() else {
                return false;
            };
            if matches!(annotation.kind, AnnotationKind::Pen { .. }) {
                return false;
            }
            self.selected = Some(id);
            self.tool = tool_for(&annotation.kind);
            self.begin_move(annotation, point);
            return true;
        }

        match self.tool {
            Tool::Select => self.selected.take().is_some(),
            Tool::Text => {
                self.begin_text(point);
                true
            }
            Tool::Emotion => {
                let emotion = self.emotion.clone();
                self.insert_emotion(point, &emotion)
            }
            Tool::Rectangle | Tool::Circle | Tool::Arrow | Tool::Pen | Tool::Mosaic => {
                self.selected = None;
                self.begin_draw(point);
                true
            }
        }
    }

    pub fn double_click(&mut self, point: Point) -> bool {
        let Some(id) = self
            .document
            .current()
            .items()
            .iter()
            .rev()
            .find(|annotation| {
                matches!(annotation.kind, AnnotationKind::Text { .. })
                    && annotation.bounds().contains(point)
            })
            .map(|annotation| annotation.id)
        else {
            return false;
        };
        let index = self
            .document
            .current()
            .get(id)
            .and_then(|annotation| match &annotation.kind {
                AnnotationKind::Text { content, .. } => Some(content.chars().count()),
                _ => None,
            })
            .unwrap_or(0);
        self.gesture = Gesture::Idle;
        self.text_baseline = Some(self.document.current().clone());
        self.selected = Some(id);
        self.hovered = Some(id);
        self.tool = Tool::Text;
        self.caret = Some(Caret {
            annotation: id,
            index,
        });
        true
    }

    pub fn pointer_move(&mut self, point: Point, region: Rect) -> bool {
        let start = match &self.gesture {
            Gesture::Idle => {
                let hovered = self.document.current().hit_test(point).filter(|id| {
                    self.document.current().get(*id).is_some_and(|annotation| {
                        !matches!(
                            annotation.kind,
                            AnnotationKind::Pen { .. } | AnnotationKind::Mosaic { .. }
                        )
                    })
                });
                if hovered == self.hovered {
                    return false;
                }
                self.hovered = hovered;
                return true;
            }
            Gesture::Drawing { start, .. }
            | Gesture::Moving { start, .. }
            | Gesture::Resizing { start, .. } => *start,
        };

        if (point.x - start.x).abs() <= DRAG_THRESHOLD
            && (point.y - start.y).abs() <= DRAG_THRESHOLD
        {
            return false;
        }

        match &mut self.gesture {
            Gesture::Idle => false,
            Gesture::Drawing { draft, .. } => {
                let point = point.clamped(region);
                match &mut draft.kind {
                    AnnotationKind::Rectangle { rect } | AnnotationKind::Circle { rect } => {
                        rect.right = point.x;
                        rect.bottom = point.y;
                    }
                    AnnotationKind::Arrow { to, .. } => *to = point,
                    AnnotationKind::Pen { points } => {
                        if points
                            .last()
                            .is_none_or(|last| last.distance(point) >= 0.75)
                        {
                            points.push(point);
                        }
                    }
                    AnnotationKind::Mosaic { points, .. } => {
                        if points
                            .last()
                            .is_none_or(|last| last.distance(point) >= 0.75)
                        {
                            points.push(point);
                            self.mosaic_generation = self.mosaic_generation.wrapping_add(1);
                        }
                    }
                    AnnotationKind::Text { .. } | AnnotationKind::Emotion { .. } => {}
                }
                true
            }
            Gesture::Moving { before, .. } => {
                let mut moved = before.clone();
                moved.translate(point.x - start.x, point.y - start.y);
                constrain_to_region(&mut moved, region);
                if let Some(annotation) = self.document.edit().get_mut(before.id) {
                    *annotation = moved;
                    true
                } else {
                    false
                }
            }
            Gesture::Resizing { handle, before, .. } => {
                let mut resized = before.clone();
                resized.resize(*handle, point.clamped(region));
                if let Some(annotation) = self.document.edit().get_mut(before.id) {
                    *annotation = resized;
                    true
                } else {
                    false
                }
            }
        }
    }

    pub fn release(&mut self) -> bool {
        match std::mem::replace(&mut self.gesture, Gesture::Idle) {
            Gesture::Idle => false,
            Gesture::Drawing { draft, .. } => {
                let mosaic = matches!(&draft.kind, AnnotationKind::Mosaic { .. });
                let selectable = !matches!(
                    &draft.kind,
                    AnnotationKind::Pen { .. } | AnnotationKind::Mosaic { .. }
                );
                if worth_keeping(&draft) {
                    let before = self.document.current().clone();
                    let id = self.document.edit().add(draft.kind, draft.stroke);
                    self.document.commit_from(before);
                    self.selected = selectable.then_some(id);
                }
                if mosaic {
                    self.mosaic_generation = self.mosaic_generation.wrapping_add(1);
                }
                true
            }
            Gesture::Moving {
                document_before, ..
            }
            | Gesture::Resizing {
                document_before, ..
            } => {
                if self.caret.is_none() {
                    self.document.commit_from(document_before);
                }
                true
            }
        }
    }

    pub fn key(&mut self, key: EditorKey) -> bool {
        if self.caret.is_some() {
            match key {
                EditorKey::Escape => {
                    self.stop_text_editing();
                    return true;
                }
                EditorKey::Enter => return self.insert_char('\n'),
                EditorKey::Backspace => return self.backspace(),
                EditorKey::Left => return self.move_caret(-1),
                EditorKey::Right => return self.move_caret(1),
                EditorKey::Up => return self.move_caret_vertical(false),
                EditorKey::Down => return self.move_caret_vertical(true),
                EditorKey::Home => return self.move_caret_to_edge(false),
                EditorKey::End => return self.move_caret_to_edge(true),
                EditorKey::Delete | EditorKey::Undo | EditorKey::Redo => {}
            }
        }

        match key {
            EditorKey::Delete => self.delete_selected(),
            EditorKey::Undo => self.undo(),
            EditorKey::Redo => self.redo(),
            _ => false,
        }
    }

    pub fn insert_char(&mut self, character: char) -> bool {
        if character.is_control() && character != '\n' {
            return false;
        }
        self.edit_text(|content, index| {
            let byte = byte_index(content, *index);
            content.insert(byte, character);
            *index += 1;
        })
    }

    pub fn clear(&mut self) {
        self.document.reset(AnnotationDocument::default());
        self.tool = Tool::Select;
        self.selected = None;
        self.hovered = None;
        self.gesture = Gesture::Idle;
        self.caret = None;
        self.text_baseline = None;
        self.mosaic_generation = self.mosaic_generation.wrapping_add(1);
    }

    fn begin_draw(&mut self, point: Point) {
        let mosaic = self.tool == Tool::Mosaic;
        let kind = match self.tool {
            Tool::Rectangle => AnnotationKind::Rectangle {
                rect: Rect::new(point.x, point.y, point.x, point.y),
            },
            Tool::Circle => AnnotationKind::Circle {
                rect: Rect::new(point.x, point.y, point.x, point.y),
            },
            Tool::Arrow => AnnotationKind::Arrow {
                from: point,
                to: point,
            },
            Tool::Pen => AnnotationKind::Pen {
                points: vec![point],
            },
            Tool::Mosaic => AnnotationKind::Mosaic {
                points: vec![point],
                block_size: self.mosaic_block_size,
            },
            Tool::Select | Tool::Text | Tool::Emotion => return,
        };
        self.gesture = Gesture::Drawing {
            start: point,
            draft: Annotation {
                id: DRAFT_ID,
                kind,
                stroke: self.stroke,
            },
        };
        if mosaic {
            self.mosaic_generation = self.mosaic_generation.wrapping_add(1);
        }
    }

    fn begin_move(&mut self, annotation: Annotation, point: Point) {
        self.gesture = Gesture::Moving {
            start: point,
            before: annotation,
            document_before: self.document.current().clone(),
        };
    }

    fn begin_resize(&mut self, annotation: Annotation, handle: Handle, point: Point) {
        self.gesture = Gesture::Resizing {
            start: point,
            handle,
            before: annotation,
            document_before: self.document.current().clone(),
        };
    }

    fn begin_text(&mut self, point: Point) {
        let baseline = self.document.current().clone();
        let id = self.document.edit().add(
            AnnotationKind::Text {
                origin: point,
                content: String::new(),
                style: self.text_style.clone(),
            },
            self.stroke,
        );
        self.selected = Some(id);
        self.caret = Some(Caret {
            annotation: id,
            index: 0,
        });
        self.text_baseline = Some(baseline);
    }

    fn stop_text_editing(&mut self) {
        let Some(caret) = self.caret.take() else {
            return;
        };
        let empty = self
            .document
            .current()
            .get(caret.annotation)
            .and_then(|annotation| match &annotation.kind {
                AnnotationKind::Text { content, .. } => Some(content.trim().is_empty()),
                _ => None,
            })
            .unwrap_or(true);
        if empty {
            self.document.edit().remove(caret.annotation);
        }
        if let Some(baseline) = self.text_baseline.take()
            && self.document.current().items() != baseline.items()
        {
            self.document.commit_from(baseline);
        }
        self.selected = None;
    }

    fn place_caret(&mut self, point: Point) {
        let Some(caret) = self.caret else {
            return;
        };
        let Some(Annotation {
            kind:
                AnnotationKind::Text {
                    origin,
                    content,
                    style,
                },
            ..
        }) = self.document.current().get(caret.annotation)
        else {
            return;
        };
        let line = ((point.y - origin.y) / (style.size * 1.2).max(1.0))
            .floor()
            .max(0.0) as usize;
        let lines: Vec<&str> = content.split('\n').collect();
        let line = line.min(lines.len().saturating_sub(1));
        let column = ((point.x - origin.x) / (style.size * 0.6).max(1.0))
            .round()
            .max(0.0) as usize;
        let offset = lines[..line]
            .iter()
            .map(|line| line.chars().count() + 1)
            .sum::<usize>();
        self.caret = Some(Caret {
            index: offset + column.min(lines[line].chars().count()),
            ..caret
        });
    }

    fn edit_text(&mut self, edit: impl FnOnce(&mut String, &mut usize)) -> bool {
        let Some(caret) = &mut self.caret else {
            return false;
        };
        let Some(annotation) = self.document.edit().get_mut(caret.annotation) else {
            return false;
        };
        let AnnotationKind::Text { content, .. } = &mut annotation.kind else {
            return false;
        };
        edit(content, &mut caret.index);
        true
    }

    fn backspace(&mut self) -> bool {
        let mut removed = false;
        self.edit_text(|content, index| {
            if *index == 0 {
                return;
            }
            let start = byte_index(content, *index - 1);
            let end = byte_index(content, *index);
            content.replace_range(start..end, "");
            *index -= 1;
            removed = true;
        });
        removed
    }

    fn move_caret(&mut self, by: isize) -> bool {
        let mut moved = false;
        self.edit_text(|content, index| {
            let next = index.saturating_add_signed(by).min(content.chars().count());
            moved = next != *index;
            *index = next;
        });
        moved
    }

    fn move_caret_to_edge(&mut self, end: bool) -> bool {
        self.edit_text(|content, index| {
            let characters: Vec<char> = content.chars().collect();
            *index = if end {
                characters[*index..]
                    .iter()
                    .position(|character| *character == '\n')
                    .map_or(characters.len(), |offset| *index + offset)
            } else {
                characters[..*index]
                    .iter()
                    .rposition(|character| *character == '\n')
                    .map_or(0, |offset| offset + 1)
            };
        })
    }

    fn move_caret_vertical(&mut self, down: bool) -> bool {
        let mut moved = false;
        self.edit_text(|content, index| {
            let characters: Vec<char> = content.chars().collect();
            let line_start = |from: usize| {
                characters[..from]
                    .iter()
                    .rposition(|character| *character == '\n')
                    .map_or(0, |position| position + 1)
            };
            let line_end = |from: usize| {
                characters[from..]
                    .iter()
                    .position(|character| *character == '\n')
                    .map_or(characters.len(), |position| from + position)
            };
            let current_start = line_start(*index);
            let column = *index - current_start;
            let target_start = if down {
                let current_end = line_end(*index);
                if current_end == characters.len() {
                    return;
                }
                current_end + 1
            } else {
                if current_start == 0 {
                    return;
                }
                line_start(current_start - 1)
            };
            *index = (target_start + column).min(line_end(target_start));
            moved = true;
        });
        moved
    }

    fn undo(&mut self) -> bool {
        self.stop_text_editing();
        let changed = self.document.undo();
        if changed {
            self.selected = None;
            self.hovered = None;
            self.mosaic_generation = self.mosaic_generation.wrapping_add(1);
        }
        changed
    }

    fn redo(&mut self) -> bool {
        self.stop_text_editing();
        if !self.document.can_redo() {
            return false;
        }
        let changed = self.document.redo();
        if changed {
            self.selected = None;
            self.hovered = None;
            self.mosaic_generation = self.mosaic_generation.wrapping_add(1);
        }
        changed
    }

    fn delete_selected(&mut self) -> bool {
        let Some(id) = self.selected else {
            return false;
        };
        let before = self.document.current().clone();
        if !self.document.edit().remove(id) {
            return false;
        }
        self.document.commit_from(before);
        self.selected = None;
        self.caret = None;
        self.text_baseline = None;
        true
    }
}

fn tool_for(kind: &AnnotationKind) -> Tool {
    match kind {
        AnnotationKind::Rectangle { .. } => Tool::Rectangle,
        AnnotationKind::Circle { .. } => Tool::Circle,
        AnnotationKind::Arrow { .. } => Tool::Arrow,
        AnnotationKind::Pen { .. } => Tool::Pen,
        AnnotationKind::Mosaic { .. } => Tool::Mosaic,
        AnnotationKind::Text { .. } => Tool::Text,
        AnnotationKind::Emotion { .. } => Tool::Emotion,
    }
}

fn constrain_to_region(annotation: &mut Annotation, region: Rect) {
    let bounds = annotation.bounds();
    let dx = if bounds.left < region.left {
        region.left - bounds.left
    } else if bounds.right > region.right {
        region.right - bounds.right
    } else {
        0.0
    };
    let dy = if bounds.top < region.top {
        region.top - bounds.top
    } else if bounds.bottom > region.bottom {
        region.bottom - bounds.bottom
    } else {
        0.0
    };
    annotation.translate(dx, dy);
}

fn worth_keeping(annotation: &Annotation) -> bool {
    match &annotation.kind {
        AnnotationKind::Rectangle { rect } | AnnotationKind::Circle { rect } => {
            rect.width() >= MIN_SHAPE && rect.height() >= MIN_SHAPE
        }
        AnnotationKind::Arrow { from, to } => from.distance(*to) >= MIN_SHAPE,
        AnnotationKind::Pen { points } | AnnotationKind::Mosaic { points, .. } => {
            points.len() >= 2
                && annotation
                    .bounds()
                    .width()
                    .max(annotation.bounds().height())
                    >= 1.0
        }
        AnnotationKind::Text { content, .. } => !content.trim().is_empty(),
        AnnotationKind::Emotion { .. } => true,
    }
}

fn byte_index(content: &str, character_index: usize) -> usize {
    content
        .char_indices()
        .nth(character_index)
        .map_or(content.len(), |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rgba;

    const REGION: Rect = Rect::new(0.0, 0.0, 800.0, 600.0);

    fn draw(editor: &mut Editor, tool: Tool, start: Point, end: Point) {
        editor.set_tool(tool);
        assert!(editor.press(start, REGION));
        assert!(editor.pointer_move(end, REGION));
        assert!(editor.release());
    }

    #[test]
    fn one_drag_is_exactly_one_undo_step() {
        let mut editor = Editor::new();
        draw(
            &mut editor,
            Tool::Rectangle,
            Point::new(10.0, 20.0),
            Point::new(110.0, 90.0),
        );
        assert_eq!(editor.annotations().items().len(), 1);
        assert!(editor.key(EditorKey::Undo));
        assert!(editor.annotations().items().is_empty());
        assert!(!editor.key(EditorKey::Undo));
        assert!(editor.key(EditorKey::Redo));
        assert_eq!(editor.annotations().items().len(), 1);
    }

    #[test]
    fn tiny_shape_is_not_committed() {
        let mut editor = Editor::new();
        editor.set_tool(Tool::Circle);
        assert!(editor.press(Point::new(10.0, 10.0), REGION));
        editor.pointer_move(Point::new(14.0, 14.0), REGION);
        editor.release();
        assert!(editor.annotations().items().is_empty());
        assert!(!editor.can_undo());
    }

    #[test]
    fn unicode_text_backspace_removes_one_character() {
        let mut editor = Editor::new();
        editor.set_tool(Tool::Text);
        editor.press(Point::new(20.0, 20.0), REGION);
        assert!(editor.insert_char('\u{4F60}'));
        assert!(editor.insert_char('\u{597D}'));
        assert!(editor.key(EditorKey::Backspace));
        let AnnotationKind::Text { content, .. } = &editor.annotations().items()[0].kind else {
            panic!("expected text annotation");
        };
        assert_eq!(content, "\u{4F60}");
    }

    #[test]
    fn double_clicking_committed_text_reopens_it_with_caret_at_end() {
        let mut editor = Editor::new();
        let origin = Point::new(80.0, 60.0);
        let content = "A\u{4F60}B";
        editor.set_tool(Tool::Text);
        assert!(editor.press(origin, REGION));
        for character in content.chars() {
            assert!(editor.insert_char(character));
        }
        assert!(editor.key(EditorKey::Escape));
        assert!(!editor.is_editing_text());

        assert!(editor.double_click(Point::new(100.0, 70.0)));
        let caret = editor.caret().expect("double-click should restore a caret");
        assert_eq!(caret.index, content.chars().count());
        assert_eq!(Some(caret.annotation), editor.selected);

        assert!(editor.insert_char('!'));
        let AnnotationKind::Text {
            content: edited, ..
        } = &editor
            .selected_annotation()
            .expect("edited text should remain selected")
            .kind
        else {
            panic!("expected text annotation");
        };
        assert_eq!(edited, "A\u{4F60}B!");
    }

    #[test]
    fn single_clicking_committed_text_selects_and_moves_without_editing() {
        let mut editor = Editor::new();
        let origin = Point::new(80.0, 60.0);
        editor.set_tool(Tool::Text);
        assert!(editor.press(origin, REGION));
        assert!(editor.insert_char('A'));
        assert!(editor.key(EditorKey::Escape));

        let click = Point::new(90.0, 70.0);
        assert!(editor.press(click, REGION));
        assert!(!editor.is_editing_text());
        assert!(editor.selected_annotation().is_some());
        assert!(editor.release());
        assert!(!editor.is_editing_text());

        assert!(editor.press(click, REGION));
        assert!(editor.pointer_move(Point::new(120.0, 100.0), REGION));
        assert!(editor.release());
        assert!(!editor.is_editing_text());
        let AnnotationKind::Text { origin: moved, .. } = &editor
            .selected_annotation()
            .expect("moved text should remain selected")
            .kind
        else {
            panic!("expected text annotation");
        };
        assert_eq!(*moved, Point::new(110.0, 90.0));
    }

    #[test]
    fn selected_shape_moves_and_stays_inside_capture() {
        let mut editor = Editor::new();
        draw(
            &mut editor,
            Tool::Rectangle,
            Point::new(10.0, 10.0),
            Point::new(110.0, 110.0),
        );
        assert!(editor.press(Point::new(50.0, 50.0), REGION));
        editor.pointer_move(Point::new(-200.0, -200.0), REGION);
        editor.release();
        assert_eq!(
            editor.annotations().items()[0].bounds(),
            Rect::new(0.0, 0.0, 100.0, 100.0)
        );
    }

    #[test]
    fn pen_is_committed_but_not_selectable() {
        let mut editor = Editor::new();
        draw(
            &mut editor,
            Tool::Pen,
            Point::new(10.0, 10.0),
            Point::new(100.0, 100.0),
        );
        assert!(editor.selected_annotation().is_none());
        editor.set_tool(Tool::Select);
        assert!(!editor.press(Point::new(50.0, 50.0), REGION));
    }

    #[test]
    fn mosaic_generation_changes_only_when_the_gpu_mask_can_change() {
        let mut editor = Editor::new();
        let initial = editor.mosaic_generation();
        draw(
            &mut editor,
            Tool::Rectangle,
            Point::new(10.0, 10.0),
            Point::new(100.0, 100.0),
        );
        assert_eq!(editor.mosaic_generation(), initial);

        editor.set_tool(Tool::Mosaic);
        assert!(editor.press(Point::new(20.0, 20.0), REGION));
        let started = editor.mosaic_generation();
        assert!(started > initial);
        assert!(editor.pointer_move(Point::new(200.0, 100.0), REGION));
        let dragged = editor.mosaic_generation();
        assert!(dragged > started);
        assert!(editor.release());
        assert!(editor.mosaic_generation() > dragged);

        let committed = editor.mosaic_generation();
        assert!(editor.key(EditorKey::Undo));
        assert!(editor.mosaic_generation() > committed);
    }

    #[test]
    fn selected_shape_color_change_is_one_undo_step() {
        let mut editor = Editor::new();
        draw(
            &mut editor,
            Tool::Rectangle,
            Point::new(10.0, 20.0),
            Point::new(110.0, 90.0),
        );
        let original = editor.annotations().items()[0].stroke;
        let mut changed = original;
        changed.color = Rgba::rgb(12, 34, 56);

        assert!(editor.set_stroke(changed));
        assert_eq!(editor.annotations().items()[0].stroke, changed);
        assert!(editor.key(EditorKey::Undo));
        assert_eq!(editor.annotations().items().len(), 1);
        assert_eq!(editor.annotations().items()[0].stroke, original);
        assert_eq!(editor.stroke(), changed);
        assert!(editor.key(EditorKey::Undo));
        assert!(editor.annotations().items().is_empty());
    }

    #[test]
    fn selected_shape_line_width_change_is_one_undo_step() {
        let mut editor = Editor::new();
        draw(
            &mut editor,
            Tool::Circle,
            Point::new(20.0, 30.0),
            Point::new(140.0, 120.0),
        );
        let original = editor.annotations().items()[0].stroke;
        let mut changed = original;
        changed.width = 8.0;

        assert!(editor.set_stroke(changed));
        assert_eq!(editor.annotations().items()[0].stroke.width, 8.0);
        assert!(editor.key(EditorKey::Undo));
        assert_eq!(editor.annotations().items().len(), 1);
        assert_eq!(editor.annotations().items()[0].stroke, original);
        assert_eq!(editor.stroke(), changed);
        assert!(editor.key(EditorKey::Undo));
        assert!(editor.annotations().items().is_empty());
    }

    #[test]
    fn selected_shape_fill_change_is_one_undo_step() {
        let mut editor = Editor::new();
        draw(
            &mut editor,
            Tool::Rectangle,
            Point::new(30.0, 40.0),
            Point::new(150.0, 130.0),
        );
        let fill = Some(Rgba::rgb(90, 80, 70).with_alpha(82));

        assert!(editor.set_fill(fill));
        assert_eq!(editor.annotations().items()[0].stroke.fill, fill);
        assert!(editor.key(EditorKey::Undo));
        assert_eq!(editor.annotations().items().len(), 1);
        assert_eq!(editor.annotations().items()[0].stroke.fill, None);
        assert_eq!(editor.stroke().fill, fill);
        assert!(editor.key(EditorKey::Undo));
        assert!(editor.annotations().items().is_empty());
    }

    #[test]
    fn selected_text_size_and_color_edits_are_independent_undo_steps() {
        let mut editor = Editor::new();
        editor.set_tool(Tool::Text);
        assert!(editor.press(Point::new(20.0, 20.0), REGION));
        assert!(editor.insert_char('A'));
        assert!(editor.key(EditorKey::Escape));

        assert!(editor.press(Point::new(32.0, 32.0), REGION));
        assert!(editor.release());
        let original = match &editor.annotations().items()[0].kind {
            AnnotationKind::Text { style, .. } => style.clone(),
            kind => panic!("expected text annotation, got {kind:?}"),
        };

        let mut resized = original.clone();
        resized.size = 32.0;
        assert!(editor.set_text_style(resized.clone()));

        let mut recolored = resized.clone();
        recolored.color = Rgba::rgb(23, 45, 67);
        assert!(editor.set_text_style(recolored.clone()));
        let AnnotationKind::Text { style, .. } = &editor.annotations().items()[0].kind else {
            panic!("expected text annotation");
        };
        assert_eq!(style, &recolored);

        assert!(editor.key(EditorKey::Undo));
        let AnnotationKind::Text { style, .. } = &editor.annotations().items()[0].kind else {
            panic!("expected text annotation");
        };
        assert_eq!(style, &resized);

        assert!(editor.key(EditorKey::Undo));
        let AnnotationKind::Text { style, .. } = &editor.annotations().items()[0].kind else {
            panic!("expected text annotation");
        };
        assert_eq!(style, &original);
        assert_eq!(editor.text_style(), &recolored);

        assert!(editor.key(EditorKey::Undo));
        assert!(editor.annotations().items().is_empty());
    }

    #[test]
    fn mosaic_block_size_is_clamped_to_four_through_sixty_four() {
        let mut editor = Editor::new();

        assert!(editor.set_mosaic_block_size(0));
        assert_eq!(editor.mosaic_block_size(), 4);
        assert!(!editor.set_mosaic_block_size(4));

        assert!(editor.set_mosaic_block_size(u32::MAX));
        assert_eq!(editor.mosaic_block_size(), 64);
        assert!(!editor.set_mosaic_block_size(65));

        assert!(editor.set_mosaic_block_size(37));
        assert_eq!(editor.mosaic_block_size(), 37);
    }

    #[test]
    fn new_mosaic_annotation_uses_the_current_block_size() {
        let mut editor = Editor::new();
        assert!(editor.set_mosaic_block_size(24));

        draw(
            &mut editor,
            Tool::Mosaic,
            Point::new(10.0, 10.0),
            Point::new(120.0, 80.0),
        );

        let AnnotationKind::Mosaic { block_size, .. } = &editor.annotations().items()[0].kind
        else {
            panic!("expected mosaic annotation");
        };
        assert_eq!(*block_size, 24);
    }

    #[test]
    fn emotion_insertion_preserves_exact_glyph_and_center_and_is_undoable() {
        let mut editor = Editor::new();
        let center = Point::new(321.5, 213.25);
        let glyph = "\u{1f469}\u{200d}\u{1f4bb}";

        assert!(editor.insert_emotion(center, glyph));
        assert_eq!(editor.tool(), Tool::Emotion);
        assert_eq!(editor.emotion(), glyph);
        assert_eq!(editor.annotations().items().len(), 1);
        let AnnotationKind::Emotion {
            center: inserted_center,
            glyph: inserted_glyph,
            ..
        } = &editor.annotations().items()[0].kind
        else {
            panic!("expected emotion annotation");
        };
        assert_eq!(*inserted_center, center);
        assert_eq!(inserted_glyph, glyph);

        assert!(editor.key(EditorKey::Undo));
        assert!(editor.annotations().items().is_empty());
        assert_eq!(editor.emotion(), glyph);
        assert!(!editor.key(EditorKey::Undo));
    }
}
