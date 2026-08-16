use super::{
    EMOTIONS, Editor, EditorKey, MOSAIC_BLOCK_SIZES, OverlayAction, OverlayOption, Point,
    PointerCursor, Rect, RectI, STROKE_WIDTHS, Selection, TEXT_SIZES, TOOLBAR_COLORS, Tool,
};

const CLICK_SLOP: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Window,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedTarget {
    /// Absolute physical-pixel coordinates on the virtual desktop.
    pub bounds: RectI,
    pub kind: TargetKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OverlayFeatures {
    pub extract_text: bool,
    pub scroll_capture: bool,
    pub languages: bool,
    pub save: bool,
    pub pin: bool,
}

#[derive(Debug)]
pub struct OverlaySession {
    desktop: RectI,
    selection: Selection,
    cursor: Option<Point>,
    highlight: Option<Rect>,
    highlight_kind: Option<TargetKind>,
    highlighter_enabled: bool,
    selection_locked: bool,
    editor: Editor,
    hovered_action: Option<OverlayAction>,
    pressed_action: Option<OverlayAction>,
    press: Option<Press>,
    features: OverlayFeatures,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    at: Point,
    mode: PressMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressMode {
    Selection,
    Manual,
    Highlight,
}

impl OverlaySession {
    #[cfg(test)]
    pub fn new(desktop: RectI) -> Self {
        Self::with_features(desktop, OverlayFeatures::default())
    }

    pub fn with_features(desktop: RectI, features: OverlayFeatures) -> Self {
        Self {
            desktop,
            selection: Selection::new(desktop.local_bounds()),
            cursor: None,
            highlight: None,
            highlight_kind: None,
            highlighter_enabled: true,
            selection_locked: false,
            editor: Editor::new(),
            hovered_action: None,
            pressed_action: None,
            press: None,
            features,
        }
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn cursor(&self) -> Option<Point> {
        self.cursor
    }

    pub fn highlight(&self) -> Option<Rect> {
        self.highlight
    }

    pub fn wants_target(&self) -> bool {
        self.highlighter_enabled && self.selection.rect().is_none() && self.press.is_none()
    }

    pub fn selection_locked(&self) -> bool {
        self.selection_locked
    }

    pub fn active_tool(&self) -> Tool {
        self.editor.tool()
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn hovered_action(&self) -> Option<OverlayAction> {
        self.hovered_action
    }

    pub fn pressed_action(&self) -> Option<OverlayAction> {
        self.pressed_action
    }

    pub fn action_enabled(&self, action: OverlayAction) -> bool {
        match action {
            OverlayAction::Option(option) => option.valid_for(self.editor.tool()),
            OverlayAction::Undo => self.editor.can_undo(),
            OverlayAction::ExtractText => self.features.extract_text,
            OverlayAction::ScrollCapture => self.features.scroll_capture,
            OverlayAction::Languages => self.features.languages,
            OverlayAction::Save => self.features.save,
            OverlayAction::Pin => self.features.pin,
            _ => true,
        }
    }

    pub fn option_active(&self, option: OverlayOption) -> bool {
        match option {
            OverlayOption::StrokeWidth(index) => STROKE_WIDTHS
                .get(index as usize)
                .is_some_and(|width| (self.editor.stroke().width - width).abs() < f32::EPSILON),
            OverlayOption::TextSize(index) => TEXT_SIZES
                .get(index as usize)
                .is_some_and(|size| (self.editor.text_style().size - size).abs() < f32::EPSILON),
            OverlayOption::ToggleFill => self.editor.stroke().fill.is_some(),
            OverlayOption::Color(index) => {
                TOOLBAR_COLORS.get(index as usize).is_some_and(|color| {
                    if self.editor.tool() == Tool::Text {
                        self.editor.text_style().color == *color
                    } else {
                        self.editor.stroke().color == *color
                    }
                })
            }
            OverlayOption::MosaicBlock(index) => MOSAIC_BLOCK_SIZES
                .get(index as usize)
                .is_some_and(|size| self.editor.mosaic_block_size() == *size),
            OverlayOption::Emotion(index) => EMOTIONS
                .get(index as usize)
                .is_some_and(|emotion| self.editor.emotion() == *emotion),
        }
    }

    pub fn set_hovered_action(&mut self, action: Option<OverlayAction>) -> bool {
        let action = action.filter(|action| self.action_enabled(*action));
        if self.hovered_action == action {
            return false;
        }
        self.hovered_action = action;
        true
    }

    pub fn press_action(&mut self, action: OverlayAction) -> bool {
        if !self.action_enabled(action) || self.pressed_action == Some(action) {
            return false;
        }
        self.pressed_action = Some(action);
        true
    }

    pub fn release_action(
        &mut self,
        pointer_action: Option<OverlayAction>,
    ) -> Option<OverlayAction> {
        let pressed = self.pressed_action.take()?;
        (Some(pressed) == pointer_action && self.action_enabled(pressed)).then_some(pressed)
    }

    pub fn activate(&mut self, action: OverlayAction) -> bool {
        if !self.action_enabled(action) {
            return false;
        }
        match action {
            OverlayAction::Tool(tool) => {
                let changed = self.editor.tool() != tool || !self.selection_locked;
                self.editor.set_tool(tool);
                self.selection_locked = true;
                changed
            }
            OverlayAction::Option(option) => {
                self.selection_locked = true;
                self.activate_option(option)
            }
            OverlayAction::Undo => self.editor.key(EditorKey::Undo),
            _ => false,
        }
    }

    pub fn nudge_selection(&mut self, dx: f32, dy: f32) -> bool {
        if self.selection_locked {
            return false;
        }
        self.selection.nudge(dx, dy)
    }

    pub fn pointer_down(&mut self, point: Point) -> bool {
        let point = point.clamped(self.desktop.local_bounds());
        let had_selection = self.selection.rect().is_some();
        if had_selection && self.selection_locked {
            return self
                .selection
                .rect()
                .is_some_and(|region| self.editor.press(point, region));
        }
        if had_selection && self.selection.hit_test(point).is_none() {
            return false;
        }
        let mode = if had_selection {
            self.selection.press(point);
            PressMode::Selection
        } else if let Some(highlight) = self.highlight.take() {
            self.highlight_kind = None;
            self.selection.set_rect(highlight);
            PressMode::Highlight
        } else {
            self.selection.press(point);
            PressMode::Manual
        };
        self.press = Some(Press { at: point, mode });
        true
    }

    pub fn double_click(&mut self, point: Point) -> bool {
        if !self.selection_locked {
            return false;
        }
        let point = point.clamped(self.desktop.local_bounds());
        self.editor.double_click(point)
    }

    pub fn pointer_cursor(&self, point: Point, toolbar_hovered: bool) -> PointerCursor {
        super::capture_cursor(self, point, toolbar_hovered)
    }

    pub fn pointer_move(&mut self, point: Point, target: Option<DetectedTarget>) -> bool {
        let point = point.clamped(self.desktop.local_bounds());
        let cursor_changed = self.cursor != Some(point);
        self.cursor = Some(point);

        if self.selection_locked {
            let changed = self
                .selection
                .rect()
                .is_some_and(|region| self.editor.pointer_move(point, region));
            return changed || (self.editor.tool() == Tool::Mosaic && cursor_changed);
        }

        if let Some(press) = self.press {
            if press.mode == PressMode::Highlight {
                if press.at.distance(point) > CLICK_SLOP {
                    self.selection.clear();
                    self.selection.press(press.at);
                    self.selection.drag(point);
                    self.press = Some(Press {
                        mode: PressMode::Manual,
                        ..press
                    });
                }
            } else {
                self.selection.drag(point);
            }
            let changed = self.highlight.take().is_some();
            self.highlight_kind = None;
            return cursor_changed || changed || self.selection.phase() != super::Phase::Ready;
        }

        if !self.wants_target() {
            return false;
        }
        let next = target.and_then(|target| {
            let clipped = target.bounds.intersection(self.desktop)?;
            Some((self.desktop_rect_to_local(clipped), target.kind))
        });
        let changed = (self.highlight, self.highlight_kind) != next.unzip();
        if let Some((bounds, kind)) = next {
            self.highlight = Some(bounds);
            self.highlight_kind = Some(kind);
        } else {
            self.highlight = None;
            self.highlight_kind = None;
        }
        cursor_changed || changed
    }

    pub fn pointer_up(&mut self, point: Point) -> bool {
        if self.selection_locked {
            self.cursor = Some(point.clamped(self.desktop.local_bounds()));
            return self.editor.release();
        }
        let Some(press) = self.press.take() else {
            return false;
        };
        let point = point.clamped(self.desktop.local_bounds());
        self.cursor = Some(point);
        let click = press.at.distance(point) <= CLICK_SLOP;
        let changed = press.mode != PressMode::Highlight && self.selection.release();
        if self.selection.rect().is_some() {
            self.highlighter_enabled = false;
            self.highlight = None;
            self.highlight_kind = None;
        }
        changed || click
    }

    pub fn clear(&mut self) {
        self.selection.clear();
        self.highlight = None;
        self.highlight_kind = None;
        self.highlighter_enabled = true;
        self.selection_locked = false;
        self.editor.clear();
        self.hovered_action = None;
        self.pressed_action = None;
        self.press = None;
    }

    pub fn select_all(&mut self) {
        self.selection.select_all();
        self.highlight = None;
        self.highlight_kind = None;
        self.highlighter_enabled = false;
        self.selection_locked = false;
        self.editor.clear();
        self.hovered_action = None;
        self.pressed_action = None;
        self.press = None;
    }

    pub fn editor_key(&mut self, key: EditorKey) -> bool {
        self.selection_locked && self.editor.key(key)
    }

    pub fn insert_character(&mut self, character: char) -> bool {
        self.selection_locked && self.editor.insert_char(character)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.desktop.width = width;
        self.desktop.height = height;
        self.selection.set_bounds(self.desktop.local_bounds());
        self.highlight = self
            .highlight
            .map(|value| value.clamped(self.desktop.local_bounds()));
    }

    fn desktop_rect_to_local(&self, rect: RectI) -> Rect {
        Rect::new(
            (rect.left - self.desktop.left) as f32,
            (rect.top - self.desktop.top) as f32,
            (rect.right() - self.desktop.left) as f32,
            (rect.bottom() - self.desktop.top) as f32,
        )
    }

    fn activate_option(&mut self, option: OverlayOption) -> bool {
        if !option.valid_for(self.editor.tool()) {
            return false;
        }
        match option {
            OverlayOption::StrokeWidth(index) => {
                let mut stroke = self.editor.stroke();
                stroke.width = STROKE_WIDTHS[index as usize];
                self.editor.set_stroke(stroke)
            }
            OverlayOption::TextSize(index) => {
                let mut style = self.editor.text_style().clone();
                style.size = TEXT_SIZES[index as usize];
                self.editor.set_text_style(style)
            }
            OverlayOption::ToggleFill => {
                let stroke = self.editor.stroke();
                let fill = stroke
                    .fill
                    .map_or(Some(stroke.color.with_alpha(82)), |_| None);
                self.editor.set_fill(fill)
            }
            OverlayOption::Color(index) => {
                let color = TOOLBAR_COLORS[index as usize];
                let mut stroke = self.editor.stroke();
                stroke.color = color;
                if stroke.fill.is_some() {
                    stroke.fill = Some(color.with_alpha(82));
                }
                let stroke_changed = self.editor.set_stroke(stroke);
                let mut style = self.editor.text_style().clone();
                style.color = color;
                self.editor.set_text_style(style) || stroke_changed
            }
            OverlayOption::MosaicBlock(index) => self
                .editor
                .set_mosaic_block_size(MOSAIC_BLOCK_SIZES[index as usize]),
            OverlayOption::Emotion(index) => {
                let Some(region) = self.selection.rect() else {
                    return false;
                };
                self.editor
                    .insert_emotion(region.center(), EMOTIONS[index as usize])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnnotationKind, Phase};

    const DESKTOP: RectI = RectI::new(-100, -50, 1000, 700);

    fn target() -> DetectedTarget {
        DetectedTarget {
            bounds: RectI::new(0, 10, 300, 200),
            kind: TargetKind::Window,
        }
    }

    #[test]
    fn detected_desktop_rect_becomes_overlay_local() {
        let mut session = OverlaySession::new(DESKTOP);
        session.pointer_move(Point::new(150.0, 100.0), Some(target()));
        assert_eq!(
            session.highlight(),
            Some(Rect::new(100.0, 60.0, 400.0, 260.0))
        );
    }

    #[test]
    fn clicking_a_highlight_adopts_it() {
        let mut session = OverlaySession::new(DESKTOP);
        let point = Point::new(150.0, 100.0);
        let expected = Some(Rect::new(100.0, 60.0, 400.0, 260.0));
        session.pointer_move(point, Some(target()));
        assert!(session.pointer_down(point));
        assert_eq!(session.selection().rect(), expected);
        assert_eq!(session.selection().phase(), Phase::Ready);
        assert_eq!(session.highlight(), None);

        assert!(session.pointer_up(point));
        assert_eq!(session.selection().rect(), expected);
        assert_eq!(session.selection().phase(), Phase::Ready);
        assert!(!session.wants_target());
    }

    #[test]
    fn dragging_empty_space_starts_a_manual_selection() {
        let mut session = OverlaySession::new(DESKTOP);
        let start = Point::new(500.0, 300.0);
        let end = Point::new(700.0, 500.0);

        assert!(session.pointer_down(start));
        assert_eq!(session.selection().phase(), Phase::Creating);
        assert!(session.pointer_move(end, None));
        assert!(session.pointer_up(end));

        assert_eq!(
            session.selection().rect(),
            Some(Rect::from_points(start, end))
        );
        assert_eq!(session.selection().phase(), Phase::Ready);
    }

    #[test]
    fn dragging_replaces_the_highlight_with_manual_selection() {
        let mut session = OverlaySession::new(DESKTOP);
        session.pointer_move(Point::new(150.0, 100.0), Some(target()));
        session.pointer_down(Point::new(150.0, 100.0));
        session.pointer_move(Point::new(500.0, 400.0), None);
        session.pointer_up(Point::new(500.0, 400.0));
        assert_eq!(
            session.selection().rect(),
            Some(Rect::new(150.0, 100.0, 500.0, 400.0))
        );
        assert_eq!(session.highlight(), None);
    }

    #[test]
    fn target_detection_stops_after_selection() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();
        session.pointer_up(Point::new(1.0, 1.0));
        assert!(!session.wants_target());
    }

    #[test]
    fn toolbar_release_must_match_the_pressed_command() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();
        assert!(session.press_action(OverlayAction::Tool(Tool::Rectangle)));
        assert_eq!(
            session.release_action(Some(OverlayAction::Tool(Tool::Circle))),
            None
        );
        assert_eq!(session.active_tool(), Tool::Select);
        assert_eq!(session.pressed_action(), None);
    }

    #[test]
    fn activating_a_tool_locks_selection_edits() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();
        let original = session.selection().rect();

        assert!(session.activate(OverlayAction::Tool(Tool::Pen)));
        assert!(session.selection_locked());
        assert_eq!(session.active_tool(), Tool::Pen);
        assert!(session.pointer_down(Point::new(200.0, 200.0)));
        assert!(session.pointer_move(Point::new(260.0, 240.0), None));
        assert!(session.pointer_up(Point::new(260.0, 240.0)));
        assert!(!session.nudge_selection(10.0, 10.0));
        assert_eq!(session.selection().rect(), original);
        assert_eq!(session.editor().annotations().items().len(), 1);
    }

    #[test]
    fn disabled_commands_cannot_be_pressed_or_hovered() {
        let mut session = OverlaySession::new(DESKTOP);
        assert!(!session.action_enabled(OverlayAction::Undo));
        assert!(!session.press_action(OverlayAction::Undo));
        for action in [
            OverlayAction::ExtractText,
            OverlayAction::ScrollCapture,
            OverlayAction::Languages,
            OverlayAction::Save,
            OverlayAction::Pin,
        ] {
            assert!(!session.action_enabled(action));
            assert!(!session.press_action(action));
            assert!(!session.set_hovered_action(Some(action)));
        }

        assert_eq!(session.hovered_action(), None);
    }

    #[test]
    fn output_commands_are_enabled_only_by_explicit_features() {
        let mut session = OverlaySession::with_features(
            DESKTOP,
            OverlayFeatures {
                save: true,
                ..OverlayFeatures::default()
            },
        );
        session.select_all();

        assert!(session.action_enabled(OverlayAction::Save));
        assert!(session.press_action(OverlayAction::Save));
        assert_eq!(
            session.release_action(Some(OverlayAction::Save)),
            Some(OverlayAction::Save)
        );
        assert!(!session.action_enabled(OverlayAction::Pin));
    }

    #[test]
    fn overlay_options_are_enabled_only_for_their_supported_tool() {
        let cases = [
            (Tool::Rectangle, OverlayOption::StrokeWidth(0), true),
            (Tool::Rectangle, OverlayOption::ToggleFill, true),
            (Tool::Rectangle, OverlayOption::Color(0), true),
            (Tool::Rectangle, OverlayOption::TextSize(0), false),
            (Tool::Arrow, OverlayOption::StrokeWidth(2), true),
            (Tool::Arrow, OverlayOption::ToggleFill, false),
            (Tool::Pen, OverlayOption::Color(6), true),
            (Tool::Text, OverlayOption::TextSize(2), true),
            (Tool::Text, OverlayOption::Color(6), true),
            (Tool::Text, OverlayOption::StrokeWidth(0), false),
            (Tool::Mosaic, OverlayOption::MosaicBlock(2), true),
            (Tool::Mosaic, OverlayOption::Color(0), false),
            (Tool::Emotion, OverlayOption::Emotion(39), true),
            (Tool::Emotion, OverlayOption::MosaicBlock(0), false),
            (Tool::Rectangle, OverlayOption::StrokeWidth(u8::MAX), false),
            (Tool::Text, OverlayOption::TextSize(u8::MAX), false),
            (Tool::Rectangle, OverlayOption::Color(u8::MAX), false),
            (Tool::Mosaic, OverlayOption::MosaicBlock(u8::MAX), false),
            (Tool::Emotion, OverlayOption::Emotion(u8::MAX), false),
        ];

        for (tool, option, expected) in cases {
            let mut session = OverlaySession::new(DESKTOP);
            session.select_all();
            assert!(session.activate(OverlayAction::Tool(tool)));
            assert_eq!(
                session.action_enabled(OverlayAction::Option(option)),
                expected,
                "unexpected enabled state for {option:?} with {tool:?}"
            );
        }
    }

    #[test]
    fn activated_overlay_options_report_their_exact_active_value() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();

        assert!(session.activate(OverlayAction::Tool(Tool::Rectangle)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::StrokeWidth(2))));
        assert!(session.option_active(OverlayOption::StrokeWidth(2)));
        assert!(!session.option_active(OverlayOption::StrokeWidth(1)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::ToggleFill)));
        assert!(session.option_active(OverlayOption::ToggleFill));
        assert!(session.activate(OverlayAction::Option(OverlayOption::Color(1))));
        assert!(session.option_active(OverlayOption::Color(1)));
        assert!(!session.option_active(OverlayOption::Color(0)));

        assert!(session.activate(OverlayAction::Tool(Tool::Text)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::TextSize(2))));
        assert!(session.option_active(OverlayOption::TextSize(2)));
        assert!(!session.option_active(OverlayOption::TextSize(1)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::Color(6))));
        assert!(session.option_active(OverlayOption::Color(6)));

        assert!(session.activate(OverlayAction::Tool(Tool::Mosaic)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::MosaicBlock(2))));
        assert!(session.option_active(OverlayOption::MosaicBlock(2)));
        assert!(!session.option_active(OverlayOption::MosaicBlock(1)));

        assert!(session.activate(OverlayAction::Tool(Tool::Emotion)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::Emotion(0))));
        assert!(session.option_active(OverlayOption::Emotion(0)));
        assert!(!session.option_active(OverlayOption::Emotion(10)));
        assert!(!session.option_active(OverlayOption::Emotion(u8::MAX)));
    }

    #[test]
    fn overlay_option_release_requires_the_same_still_enabled_option() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();
        assert!(session.activate(OverlayAction::Tool(Tool::Rectangle)));
        let width = OverlayAction::Option(OverlayOption::StrokeWidth(0));
        let color = OverlayAction::Option(OverlayOption::Color(0));
        let text_size = OverlayAction::Option(OverlayOption::TextSize(0));

        assert!(session.press_action(width));
        assert_eq!(session.pressed_action(), Some(width));
        assert!(!session.press_action(width));
        assert_eq!(session.release_action(Some(width)), Some(width));
        assert_eq!(session.pressed_action(), None);

        assert!(session.press_action(width));
        assert_eq!(session.release_action(Some(color)), None);
        assert_eq!(session.pressed_action(), None);

        assert!(!session.press_action(text_size));
        assert_eq!(session.pressed_action(), None);

        assert!(session.press_action(width));
        assert!(session.activate(OverlayAction::Tool(Tool::Text)));
        assert_eq!(session.release_action(Some(width)), None);
        assert_eq!(session.pressed_action(), None);
    }

    #[test]
    fn shape_options_are_applied_to_the_next_annotation() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();
        assert!(session.activate(OverlayAction::Tool(Tool::Rectangle)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::StrokeWidth(2))));
        assert!(session.activate(OverlayAction::Option(OverlayOption::Color(1))));
        assert!(session.activate(OverlayAction::Option(OverlayOption::ToggleFill)));

        assert!(session.pointer_down(Point::new(100.0, 100.0)));
        assert!(session.pointer_move(Point::new(240.0, 180.0), None));
        assert!(session.pointer_up(Point::new(240.0, 180.0)));

        let annotation = &session.editor().annotations().items()[0];
        assert_eq!(annotation.stroke.width, 8.0);
        assert_eq!(annotation.stroke.color, TOOLBAR_COLORS[1]);
        assert_eq!(
            annotation.stroke.fill,
            Some(TOOLBAR_COLORS[1].with_alpha(82))
        );
    }

    #[test]
    fn text_and_mosaic_options_reach_their_new_annotations() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();
        assert!(session.activate(OverlayAction::Tool(Tool::Text)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::TextSize(2))));
        assert!(session.activate(OverlayAction::Option(OverlayOption::Color(6))));
        assert!(session.pointer_down(Point::new(100.0, 100.0)));
        assert!(session.insert_character('A'));
        assert!(session.editor_key(EditorKey::Escape));

        let AnnotationKind::Text { style, .. } = &session.editor().annotations().items()[0].kind
        else {
            panic!("expected text annotation");
        };
        assert_eq!(style.size, 32.0);
        assert_eq!(style.color, TOOLBAR_COLORS[6]);

        assert!(session.activate(OverlayAction::Tool(Tool::Mosaic)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::MosaicBlock(2))));
        assert!(session.pointer_down(Point::new(200.0, 200.0)));
        assert!(session.pointer_move(Point::new(300.0, 240.0), None));
        assert!(session.pointer_up(Point::new(300.0, 240.0)));
        let AnnotationKind::Mosaic { block_size, .. } =
            &session.editor().annotations().items()[1].kind
        else {
            panic!("expected mosaic annotation");
        };
        assert_eq!(*block_size, 24);
    }

    #[test]
    fn emotion_option_inserts_the_exact_glyph_at_selection_center() {
        let mut session = OverlaySession::new(DESKTOP);
        session.select_all();
        assert!(session.activate(OverlayAction::Tool(Tool::Emotion)));
        assert!(session.activate(OverlayAction::Option(OverlayOption::Emotion(39))));

        let annotation = &session.editor().annotations().items()[0];
        let AnnotationKind::Emotion { center, glyph, .. } = &annotation.kind else {
            panic!("expected emotion annotation");
        };
        assert_eq!(*center, DESKTOP.local_bounds().center());
        assert_eq!(glyph, EMOTIONS[39]);
    }
}
