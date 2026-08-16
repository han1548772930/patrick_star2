use super::{AnnotationKind, Handle, OverlaySession, Phase, Point, Tool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerCursor {
    Arrow,
    Crosshair,
    Hand,
    IBeam,
    Move,
    ResizeNorthSouth,
    ResizeEastWest,
    ResizeNorthEastSouthWest,
    ResizeNorthWestSouthEast,
    NotAllowed,
    Hidden,
}

pub const fn resize_cursor(handle: Handle) -> PointerCursor {
    match handle {
        Handle::TopLeft | Handle::BottomRight | Handle::Start | Handle::End => {
            PointerCursor::ResizeNorthWestSouthEast
        }
        Handle::TopRight | Handle::BottomLeft => PointerCursor::ResizeNorthEastSouthWest,
        Handle::Top | Handle::Bottom => PointerCursor::ResizeNorthSouth,
        Handle::Left | Handle::Right => PointerCursor::ResizeEastWest,
        Handle::Move => PointerCursor::Move,
    }
}

pub fn capture_cursor(
    session: &OverlaySession,
    point: Point,
    toolbar_hovered: bool,
) -> PointerCursor {
    if toolbar_hovered {
        return PointerCursor::Hand;
    }

    match session.selection().phase() {
        Phase::Creating => return PointerCursor::Crosshair,
        Phase::Moving => return PointerCursor::Move,
        Phase::Resizing => {
            return session
                .selection()
                .drag_handle()
                .map_or(PointerCursor::Arrow, resize_cursor);
        }
        Phase::Idle => return PointerCursor::Arrow,
        Phase::Ready => {}
    }

    let editor = session.editor();
    if let Some(handle) = editor.drag_handle() {
        return resize_cursor(handle);
    }
    if editor.is_editing_text() {
        return editor
            .grip_under(point)
            .filter(|handle| *handle != Handle::Move)
            .map_or(PointerCursor::IBeam, resize_cursor);
    }
    if !session.selection_locked()
        && let Some(handle) = session
            .selection()
            .hit_test(point)
            .filter(|handle| *handle != Handle::Move)
    {
        return resize_cursor(handle);
    }
    if let Some(handle) = editor.grip_under(point) {
        if handle != Handle::Move {
            return resize_cursor(handle);
        }
        return if editor
            .selected_annotation()
            .is_some_and(|annotation| matches!(annotation.kind, AnnotationKind::Text { .. }))
        {
            PointerCursor::Arrow
        } else {
            PointerCursor::Move
        };
    }

    let region = session
        .selection()
        .rect()
        .expect("a ready selection has a rectangle");
    let inside = region.contains(point);
    let tool = session.active_tool();
    if inside {
        return match tool {
            Tool::Rectangle | Tool::Circle | Tool::Arrow | Tool::Pen => PointerCursor::Crosshair,
            Tool::Text => PointerCursor::IBeam,
            Tool::Mosaic => PointerCursor::Hidden,
            Tool::Emotion | Tool::Select => PointerCursor::Arrow,
        };
    }
    if matches!(
        tool,
        Tool::Rectangle | Tool::Circle | Tool::Arrow | Tool::Pen | Tool::Text | Tool::Mosaic
    ) {
        PointerCursor::NotAllowed
    } else {
        PointerCursor::Arrow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EditorKey, OverlayAction, OverlayFeatures, RectI};

    fn selected_session() -> OverlaySession {
        let mut session =
            OverlaySession::with_features(RectI::new(0, 0, 800, 600), OverlayFeatures::default());
        session.select_all();
        session
    }

    #[test]
    fn resize_handles_map_to_native_cursor_semantics() {
        assert_eq!(
            resize_cursor(Handle::TopLeft),
            PointerCursor::ResizeNorthWestSouthEast
        );
        assert_eq!(
            resize_cursor(Handle::TopRight),
            PointerCursor::ResizeNorthEastSouthWest
        );
        assert_eq!(resize_cursor(Handle::Top), PointerCursor::ResizeNorthSouth);
        assert_eq!(resize_cursor(Handle::Right), PointerCursor::ResizeEastWest);
        assert_eq!(resize_cursor(Handle::Start), resize_cursor(Handle::End));
    }

    #[test]
    fn toolbar_and_drawing_tools_use_distinct_cursor_semantics() {
        let mut session = selected_session();
        session.activate(OverlayAction::Tool(Tool::Text));
        assert_eq!(
            session.pointer_cursor(Point::new(300.0, 300.0), true),
            PointerCursor::Hand
        );
        assert_eq!(
            session.pointer_cursor(Point::new(300.0, 300.0), false),
            PointerCursor::IBeam
        );
        assert_eq!(
            session.pointer_cursor(Point::new(900.0, 300.0), false),
            PointerCursor::NotAllowed
        );
    }

    #[test]
    fn mosaic_hides_only_the_canvas_cursor_inside_the_capture() {
        let mut session = selected_session();
        session.activate(OverlayAction::Tool(Tool::Mosaic));
        assert_eq!(
            session.pointer_cursor(Point::new(300.0, 300.0), false),
            PointerCursor::Hidden
        );
        assert_eq!(
            session.pointer_cursor(Point::new(300.0, 300.0), true),
            PointerCursor::Hand
        );
    }

    #[test]
    fn committed_text_uses_an_ibeam_after_double_click_reopens_editing() {
        let mut session = selected_session();
        let origin = Point::new(100.0, 100.0);
        assert!(session.activate(OverlayAction::Tool(Tool::Text)));
        assert!(session.pointer_down(origin));
        for character in "hello".chars() {
            assert!(session.insert_character(character));
        }
        assert!(session.editor_key(EditorKey::Escape));

        assert!(session.double_click(Point::new(120.0, 110.0)));
        assert!(session.editor().is_editing_text());
        assert_eq!(
            session.pointer_cursor(Point::new(120.0, 110.0), false),
            PointerCursor::IBeam
        );
    }
}
