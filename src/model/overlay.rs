use super::{Point, Rect, Rgba, Tool};

const BAR_HEIGHT: f32 = 40.0;
const BAR_GAP: f32 = 3.0;
const BAR_PADDING: f32 = 8.0;
const BUTTON_SIZE: f32 = 30.0;
const BUTTON_GAP: f32 = 4.0;
const OPTIONS_GAP: f32 = 3.0;
const OPTIONS_PADDING: f32 = 8.0;
const OPTION_BUTTON_SIZE: f32 = 28.0;
const OPTION_BUTTON_GAP: f32 = 2.0;

pub const STROKE_WIDTHS: [f32; 3] = [2.0, 4.0, 8.0];
pub const TEXT_SIZES: [f32; 3] = [12.0, 20.0, 32.0];
pub const MOSAIC_BLOCK_SIZES: [u32; 3] = [10, 16, 24];
pub const TOOLBAR_COLORS: [Rgba; 7] = [
    Rgba::rgb(0x00, 0x78, 0xd4),
    Rgba::rgb(0x10, 0x89, 0x3e),
    Rgba::rgb(0xff, 0xd7, 0x00),
    Rgba::rgb(0x50, 0x50, 0x50),
    Rgba::WHITE,
    Rgba::rgb(0xe8, 0x11, 0x23),
    Rgba::BLACK,
];
pub const EMOTIONS: [&str; 40] = [
    "\u{1f600}",
    "\u{1f603}",
    "\u{1f604}",
    "\u{1f601}",
    "\u{1f606}",
    "\u{1f605}",
    "\u{1f602}",
    "\u{1f923}",
    "\u{1f60a}",
    "\u{1f607}",
    "\u{1f642}",
    "\u{1f643}",
    "\u{1f609}",
    "\u{1f60c}",
    "\u{1f60d}",
    "\u{1f970}",
    "\u{1f618}",
    "\u{1f617}",
    "\u{1f619}",
    "\u{1f61a}",
    "\u{1f60b}",
    "\u{1f61b}",
    "\u{1f61d}",
    "\u{1f61c}",
    "\u{1f914}",
    "\u{1f917}",
    "\u{1f929}",
    "\u{1f60e}",
    "\u{1f973}",
    "\u{1f622}",
    "\u{1f62d}",
    "\u{1f621}",
    "\u{1f620}",
    "\u{1f631}",
    "\u{1f634}",
    "\u{1f92f}",
    "\u{1f44d}",
    "\u{1f44f}",
    "\u{1f389}",
    "\u{2b50}",
];

pub const CAPTURE_ACTIONS: [OverlayAction; 15] = [
    OverlayAction::Tool(Tool::Rectangle),
    OverlayAction::Tool(Tool::Circle),
    OverlayAction::Tool(Tool::Emotion),
    OverlayAction::Tool(Tool::Arrow),
    OverlayAction::Tool(Tool::Pen),
    OverlayAction::Tool(Tool::Mosaic),
    OverlayAction::Tool(Tool::Text),
    OverlayAction::Undo,
    OverlayAction::ExtractText,
    OverlayAction::ScrollCapture,
    OverlayAction::Languages,
    OverlayAction::Save,
    OverlayAction::Pin,
    OverlayAction::Confirm,
    OverlayAction::Cancel,
];

pub const SCROLL_ACTIONS: [ScrollAction; 4] = [
    ScrollAction::Edit,
    ScrollAction::Save,
    ScrollAction::Cancel,
    ScrollAction::Confirm,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Tool(Tool),
    Option(OverlayOption),
    Undo,
    ExtractText,
    ScrollCapture,
    Languages,
    Save,
    Pin,
    Cancel,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAction {
    Edit,
    Save,
    Cancel,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayOption {
    StrokeWidth(u8),
    TextSize(u8),
    ToggleFill,
    Color(u8),
    MosaicBlock(u8),
    Emotion(u8),
}

impl OverlayOption {
    pub fn valid_for(self, tool: Tool) -> bool {
        match self {
            Self::StrokeWidth(index) => {
                matches!(
                    tool,
                    Tool::Rectangle | Tool::Circle | Tool::Arrow | Tool::Pen
                ) && STROKE_WIDTHS.get(index as usize).is_some()
            }
            Self::TextSize(index) => tool == Tool::Text && TEXT_SIZES.get(index as usize).is_some(),
            Self::ToggleFill => matches!(tool, Tool::Rectangle | Tool::Circle),
            Self::Color(index) => {
                matches!(
                    tool,
                    Tool::Rectangle | Tool::Circle | Tool::Arrow | Tool::Pen | Tool::Text
                ) && TOOLBAR_COLORS.get(index as usize).is_some()
            }
            Self::MosaicBlock(index) => {
                tool == Tool::Mosaic && MOSAIC_BLOCK_SIZES.get(index as usize).is_some()
            }
            Self::Emotion(index) => tool == Tool::Emotion && EMOTIONS.get(index as usize).is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActionButton {
    pub action: OverlayAction,
    pub bounds: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayLayout {
    pub bar: Rect,
    pub buttons: [ActionButton; CAPTURE_ACTIONS.len()],
    pub options: Option<OptionsLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollActionButton {
    pub action: ScrollAction,
    pub bounds: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollLayout {
    pub bar: Rect,
    pub buttons: [ScrollActionButton; SCROLL_ACTIONS.len()],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionsLayout {
    pub bar: Rect,
    tool: Tool,
    columns: usize,
    rows: usize,
    button_size: f32,
    gap: f32,
    padding: f32,
}

impl OverlayLayout {
    pub fn for_tool(selection: Rect, surface: Rect, tool: Tool) -> Self {
        let (bar, padding, button_gap, button_size) =
            action_bar(selection, surface, CAPTURE_ACTIONS.len());
        let buttons = std::array::from_fn(|index| {
            let button_left = bar.left + padding + index as f32 * (button_size + button_gap);
            ActionButton {
                action: CAPTURE_ACTIONS[index],
                bounds: Rect::new(
                    button_left,
                    bar.top + (bar.height() - button_size) * 0.5,
                    button_left + button_size,
                    bar.top + (bar.height() + button_size) * 0.5,
                ),
            }
        });
        let options = OptionsLayout::new(tool, bar, selection, surface);
        Self {
            bar,
            buttons,
            options,
        }
    }

    pub fn action_at(self, point: Point) -> Option<OverlayAction> {
        self.options
            .and_then(|options| options.action_at(point))
            .or_else(|| {
                self.buttons
                    .iter()
                    .find(|button| button.bounds.contains(point))
                    .map(|button| button.action)
            })
    }
}

impl ScrollLayout {
    pub fn new(selection: Rect, surface: Rect) -> Self {
        let (bar, padding, button_gap, button_size) =
            action_bar(selection, surface, SCROLL_ACTIONS.len());
        let buttons = std::array::from_fn(|index| {
            let button_left = bar.left + padding + index as f32 * (button_size + button_gap);
            ScrollActionButton {
                action: SCROLL_ACTIONS[index],
                bounds: Rect::new(
                    button_left,
                    bar.top + (bar.height() - button_size) * 0.5,
                    button_left + button_size,
                    bar.top + (bar.height() + button_size) * 0.5,
                ),
            }
        });
        Self { bar, buttons }
    }

    pub fn action_at(self, point: Point) -> Option<ScrollAction> {
        self.buttons
            .iter()
            .find(|button| button.bounds.contains(point))
            .map(|button| button.action)
    }
}

fn action_bar(selection: Rect, surface: Rect, action_count: usize) -> (Rect, f32, f32, f32) {
    let count = action_count as f32;
    let nominal_width = BAR_PADDING * 2.0
        + BUTTON_SIZE * count
        + BUTTON_GAP * action_count.saturating_sub(1) as f32;
    let bar_width = nominal_width.min(surface.width().max(0.0));
    let bar_height = BAR_HEIGHT.min(surface.height().max(0.0));
    let padding = BAR_PADDING.min(bar_width * 0.05);
    let gap_budget = (bar_width - padding * 2.0 - count * 12.0).max(0.0);
    let button_gap = if action_count > 1 {
        BUTTON_GAP.min(gap_budget / (action_count - 1) as f32)
    } else {
        0.0
    };
    let button_size =
        ((bar_width - padding * 2.0 - button_gap * action_count.saturating_sub(1) as f32)
            / count.max(1.0))
        .clamp(0.0, BUTTON_SIZE)
        .min(bar_height);
    let centered = (selection.left + selection.right - bar_width) * 0.5;
    let left = centered
        .max(surface.left)
        .min((surface.right - bar_width).max(surface.left));
    let below = selection.bottom + BAR_GAP;
    let above = selection.top - BAR_GAP - bar_height;
    let top = if below + bar_height <= surface.bottom {
        below
    } else {
        above.max(surface.top)
    }
    .min((surface.bottom - bar_height).max(surface.top));
    (
        Rect::new(left, top, left + bar_width, top + bar_height),
        padding,
        button_gap,
        button_size,
    )
}

impl OptionsLayout {
    fn new(tool: Tool, anchor: Rect, selection: Rect, surface: Rect) -> Option<Self> {
        let (count, columns) = option_grid(tool)?;
        let rows = count.div_ceil(columns);
        let padding = OPTIONS_PADDING
            .min(surface.width() * 0.05)
            .min(surface.height() * 0.05);
        let gap = OPTION_BUTTON_GAP
            .min(((surface.width() - padding * 2.0) / columns as f32).max(0.0) * 0.2);
        let width_for_buttons =
            (surface.width() - padding * 2.0 - gap * columns.saturating_sub(1) as f32).max(0.0);
        let height_for_buttons =
            (surface.height() - padding * 2.0 - gap * rows.saturating_sub(1) as f32).max(0.0);
        let button_size = OPTION_BUTTON_SIZE
            .min(width_for_buttons / columns as f32)
            .min(height_for_buttons / rows as f32)
            .max(0.0);
        let width =
            padding * 2.0 + button_size * columns as f32 + gap * columns.saturating_sub(1) as f32;
        let height =
            padding * 2.0 + button_size * rows as f32 + gap * rows.saturating_sub(1) as f32;
        let left = (anchor.center().x - width * 0.5)
            .max(surface.left)
            .min((surface.right - width).max(surface.left));
        let prefer_below = anchor.top >= selection.bottom;
        let below = anchor.bottom + OPTIONS_GAP;
        let above = anchor.top - OPTIONS_GAP - height;
        let preferred = if prefer_below { below } else { above };
        let alternate = if prefer_below { above } else { below };
        let fits = |top: f32| top >= surface.top && top + height <= surface.bottom;
        let top = if fits(preferred) {
            preferred
        } else if fits(alternate) {
            alternate
        } else {
            preferred
                .max(surface.top)
                .min((surface.bottom - height).max(surface.top))
        };
        Some(Self {
            bar: Rect::new(left, top, left + width, top + height),
            tool,
            columns,
            rows,
            button_size,
            gap,
            padding,
        })
    }

    pub fn option_count(self) -> usize {
        option_grid(self.tool).map_or(0, |(count, _)| count)
    }

    pub fn buttons(self) -> impl Iterator<Item = ActionButton> {
        (0..self.option_count()).filter_map(move |index| self.button(index))
    }

    pub fn button(self, index: usize) -> Option<ActionButton> {
        let option = option_at(self.tool, index)?;
        let row = index / self.columns;
        if row >= self.rows {
            return None;
        }
        let column = index % self.columns;
        let left = self.bar.left + self.padding + column as f32 * (self.button_size + self.gap);
        let top = self.bar.top + self.padding + row as f32 * (self.button_size + self.gap);
        Some(ActionButton {
            action: OverlayAction::Option(option),
            bounds: Rect::new(left, top, left + self.button_size, top + self.button_size),
        })
    }

    pub fn action_at(self, point: Point) -> Option<OverlayAction> {
        if !self.bar.contains(point) {
            return None;
        }
        self.buttons()
            .find(|button| button.bounds.contains(point))
            .map(|button| button.action)
    }
}

fn option_grid(tool: Tool) -> Option<(usize, usize)> {
    match tool {
        Tool::Rectangle | Tool::Circle => Some((11, 11)),
        Tool::Arrow | Tool::Pen | Tool::Text => Some((10, 10)),
        Tool::Mosaic => Some((3, 3)),
        Tool::Emotion => Some((EMOTIONS.len(), 8)),
        Tool::Select => None,
    }
}

fn option_at(tool: Tool, index: usize) -> Option<OverlayOption> {
    match tool {
        Tool::Rectangle | Tool::Circle => match index {
            0..=2 => Some(OverlayOption::StrokeWidth(index as u8)),
            3 => Some(OverlayOption::ToggleFill),
            4..=10 => Some(OverlayOption::Color((index - 4) as u8)),
            _ => None,
        },
        Tool::Arrow | Tool::Pen => match index {
            0..=2 => Some(OverlayOption::StrokeWidth(index as u8)),
            3..=9 => Some(OverlayOption::Color((index - 3) as u8)),
            _ => None,
        },
        Tool::Text => match index {
            0..=2 => Some(OverlayOption::TextSize(index as u8)),
            3..=9 => Some(OverlayOption::Color((index - 3) as u8)),
            _ => None,
        },
        Tool::Mosaic => {
            (index < MOSAIC_BLOCK_SIZES.len()).then_some(OverlayOption::MosaicBlock(index as u8))
        }
        Tool::Emotion => (index < EMOTIONS.len()).then_some(OverlayOption::Emotion(index as u8)),
        Tool::Select => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: Rect = Rect::new(0.0, 0.0, 800.0, 600.0);
    const SHAPE_OPTIONS: [OverlayOption; 11] = [
        OverlayOption::StrokeWidth(0),
        OverlayOption::StrokeWidth(1),
        OverlayOption::StrokeWidth(2),
        OverlayOption::ToggleFill,
        OverlayOption::Color(0),
        OverlayOption::Color(1),
        OverlayOption::Color(2),
        OverlayOption::Color(3),
        OverlayOption::Color(4),
        OverlayOption::Color(5),
        OverlayOption::Color(6),
    ];
    const STROKE_OPTIONS: [OverlayOption; 10] = [
        OverlayOption::StrokeWidth(0),
        OverlayOption::StrokeWidth(1),
        OverlayOption::StrokeWidth(2),
        OverlayOption::Color(0),
        OverlayOption::Color(1),
        OverlayOption::Color(2),
        OverlayOption::Color(3),
        OverlayOption::Color(4),
        OverlayOption::Color(5),
        OverlayOption::Color(6),
    ];
    const TEXT_OPTIONS: [OverlayOption; 10] = [
        OverlayOption::TextSize(0),
        OverlayOption::TextSize(1),
        OverlayOption::TextSize(2),
        OverlayOption::Color(0),
        OverlayOption::Color(1),
        OverlayOption::Color(2),
        OverlayOption::Color(3),
        OverlayOption::Color(4),
        OverlayOption::Color(5),
        OverlayOption::Color(6),
    ];
    const MOSAIC_OPTIONS: [OverlayOption; 3] = [
        OverlayOption::MosaicBlock(0),
        OverlayOption::MosaicBlock(1),
        OverlayOption::MosaicBlock(2),
    ];
    const CONTEXT_TOOLS: [Tool; 7] = [
        Tool::Rectangle,
        Tool::Circle,
        Tool::Arrow,
        Tool::Pen,
        Tool::Text,
        Tool::Mosaic,
        Tool::Emotion,
    ];

    fn context_layout(tool: Tool, selection: Rect, surface: Rect) -> OverlayLayout {
        OverlayLayout::for_tool(selection, surface, tool)
    }

    fn options_for(tool: Tool) -> OptionsLayout {
        context_layout(tool, Rect::new(100.0, 100.0, 500.0, 300.0), SURFACE)
            .options
            .expect("tool should have context options")
    }

    fn assert_rect_inside(inner: Rect, outer: Rect) {
        assert!(
            inner.left >= outer.left,
            "{inner:?} extends left of {outer:?}"
        );
        assert!(inner.top >= outer.top, "{inner:?} extends above {outer:?}");
        assert!(
            inner.right <= outer.right,
            "{inner:?} extends right of {outer:?}"
        );
        assert!(
            inner.bottom <= outer.bottom,
            "{inner:?} extends below {outer:?}"
        );
    }

    #[test]
    fn bar_prefers_below_and_aligns_to_selection_right() {
        let layout =
            OverlayLayout::for_tool(Rect::new(100.0, 100.0, 500.0, 300.0), SURFACE, Tool::Select);
        assert_eq!(layout.bar, Rect::new(39.0, 303.0, 561.0, 343.0));
    }

    #[test]
    fn bar_moves_above_near_bottom_edge() {
        let layout =
            OverlayLayout::for_tool(Rect::new(100.0, 500.0, 780.0, 590.0), SURFACE, Tool::Select);
        assert_eq!(layout.bar.top, 457.0);
        assert!(layout.bar.right <= SURFACE.right);
    }

    #[test]
    fn action_hit_testing_uses_the_drawn_button_bounds() {
        let layout =
            OverlayLayout::for_tool(Rect::new(100.0, 100.0, 500.0, 300.0), SURFACE, Tool::Select);
        let rectangle = layout.buttons[0].bounds;
        let cancel = layout.buttons[14].bounds;
        assert_eq!(
            layout.action_at(Point::new(rectangle.left + 4.0, rectangle.top + 4.0)),
            Some(OverlayAction::Tool(Tool::Rectangle))
        );
        assert_eq!(
            layout.action_at(Point::new(cancel.left + 4.0, cancel.top + 4.0)),
            Some(OverlayAction::Cancel)
        );
    }

    #[test]
    fn capture_actions_keep_the_original_complete_order() {
        assert_eq!(
            CAPTURE_ACTIONS,
            [
                OverlayAction::Tool(Tool::Rectangle),
                OverlayAction::Tool(Tool::Circle),
                OverlayAction::Tool(Tool::Emotion),
                OverlayAction::Tool(Tool::Arrow),
                OverlayAction::Tool(Tool::Pen),
                OverlayAction::Tool(Tool::Mosaic),
                OverlayAction::Tool(Tool::Text),
                OverlayAction::Undo,
                OverlayAction::ExtractText,
                OverlayAction::ScrollCapture,
                OverlayAction::Languages,
                OverlayAction::Save,
                OverlayAction::Pin,
                OverlayAction::Confirm,
                OverlayAction::Cancel,
            ]
        );
    }

    #[test]
    fn scrolling_actions_keep_the_original_complete_order() {
        assert_eq!(
            SCROLL_ACTIONS,
            [
                ScrollAction::Edit,
                ScrollAction::Save,
                ScrollAction::Cancel,
                ScrollAction::Confirm,
            ]
        );
    }

    #[test]
    fn scrolling_action_hit_testing_uses_each_drawn_button() {
        let layout = ScrollLayout::new(Rect::new(100.0, 100.0, 500.0, 300.0), SURFACE);
        for button in layout.buttons {
            assert_eq!(
                layout.action_at(button.bounds.center()),
                Some(button.action)
            );
        }
        assert_eq!(layout.action_at(Point::new(0.0, 0.0)), None);
    }

    #[test]
    fn scrolling_toolbar_avoids_surface_edges() {
        for selection in [
            Rect::new(0.0, 0.0, 80.0, 40.0),
            Rect::new(720.0, 540.0, 800.0, 600.0),
        ] {
            let layout = ScrollLayout::new(selection, SURFACE);
            assert_rect_inside(layout.bar, SURFACE);
            assert!(
                layout
                    .buttons
                    .iter()
                    .all(|button| SURFACE.contains(button.bounds.center()))
            );
        }
    }

    #[test]
    fn toolbar_and_buttons_stay_inside_a_narrow_surface() {
        let surface = Rect::new(0.0, 0.0, 320.0, 200.0);
        let layout =
            OverlayLayout::for_tool(Rect::new(280.0, 20.0, 320.0, 80.0), surface, Tool::Select);
        assert_eq!(layout.bar.left, surface.left);
        assert_eq!(layout.bar.right, surface.right);
        assert!(layout.buttons.iter().all(
            |button| button.bounds.left >= surface.left && button.bounds.right <= surface.right
        ));
    }

    #[test]
    fn context_options_have_complete_command_order() {
        for tool in [Tool::Rectangle, Tool::Circle] {
            let options = options_for(tool);
            let actual: Vec<_> = options.buttons().map(|button| button.action).collect();
            let expected: Vec<_> = SHAPE_OPTIONS
                .iter()
                .copied()
                .map(OverlayAction::Option)
                .collect();
            assert_eq!(options.option_count(), 11, "wrong count for {tool:?}");
            assert_eq!(actual, expected, "wrong command order for {tool:?}");
            assert!(options.button(11).is_none());
        }

        for tool in [Tool::Arrow, Tool::Pen] {
            let options = options_for(tool);
            let actual: Vec<_> = options.buttons().map(|button| button.action).collect();
            let expected: Vec<_> = STROKE_OPTIONS
                .iter()
                .copied()
                .map(OverlayAction::Option)
                .collect();
            assert_eq!(options.option_count(), 10, "wrong count for {tool:?}");
            assert_eq!(actual, expected, "wrong command order for {tool:?}");
            assert!(options.button(10).is_none());
        }

        let text = options_for(Tool::Text);
        let actual: Vec<_> = text.buttons().map(|button| button.action).collect();
        let expected: Vec<_> = TEXT_OPTIONS
            .iter()
            .copied()
            .map(OverlayAction::Option)
            .collect();
        assert_eq!(text.option_count(), 10);
        assert_eq!(actual, expected, "wrong command order for Text");
        assert!(text.button(10).is_none());

        let mosaic = options_for(Tool::Mosaic);
        let actual: Vec<_> = mosaic.buttons().map(|button| button.action).collect();
        let expected: Vec<_> = MOSAIC_OPTIONS
            .iter()
            .copied()
            .map(OverlayAction::Option)
            .collect();
        assert_eq!(mosaic.option_count(), 3);
        assert_eq!(actual, expected, "wrong command order for Mosaic");
        assert!(mosaic.button(3).is_none());

        let emotion = options_for(Tool::Emotion);
        let actual: Vec<_> = emotion.buttons().map(|button| button.action).collect();
        let expected: Vec<_> = (0..40)
            .map(|index| OverlayAction::Option(OverlayOption::Emotion(index)))
            .collect();
        assert_eq!(emotion.option_count(), 40);
        assert_eq!(actual, expected, "wrong command order for Emotion");
        assert!(emotion.button(40).is_none());
    }

    #[test]
    fn context_option_hit_testing_uses_actual_button_bounds() {
        for tool in CONTEXT_TOOLS {
            let layout = context_layout(tool, Rect::new(100.0, 100.0, 500.0, 300.0), SURFACE);
            let options = layout.options.expect("tool should have context options");

            for button in options.buttons() {
                assert_eq!(
                    layout.action_at(button.bounds.center()),
                    Some(button.action),
                    "failed to hit {tool:?} button {button:?}"
                );
            }
        }
    }

    #[test]
    fn options_bar_stays_inside_surface_near_top_and_bottom_edges() {
        let selections = [
            Rect::new(200.0, 0.0, 600.0, 20.0),
            Rect::new(200.0, 580.0, 600.0, 600.0),
        ];

        for tool in CONTEXT_TOOLS {
            for selection in selections {
                let options = context_layout(tool, selection, SURFACE)
                    .options
                    .expect("tool should have context options");
                assert_rect_inside(options.bar, SURFACE);
            }
        }
    }

    #[test]
    fn narrow_surface_option_buttons_stay_inside_without_overlap() {
        let surface = Rect::new(-60.0, -20.0, 60.0, 160.0);
        let selection = Rect::new(-50.0, 30.0, 50.0, 80.0);

        for tool in CONTEXT_TOOLS {
            let options = context_layout(tool, selection, surface)
                .options
                .expect("tool should have context options");
            let buttons: Vec<_> = options.buttons().collect();

            assert_rect_inside(options.bar, surface);
            assert_eq!(buttons.len(), options.option_count());
            for button in &buttons {
                assert_rect_inside(button.bounds, surface);
                assert!(button.bounds.width() > 0.0);
                assert!(button.bounds.height() > 0.0);
            }

            for (index, first) in buttons.iter().enumerate() {
                for second in &buttons[index + 1..] {
                    let separated = first.bounds.right <= second.bounds.left
                        || second.bounds.right <= first.bounds.left
                        || first.bounds.bottom <= second.bounds.top
                        || second.bounds.bottom <= first.bounds.top;
                    assert!(
                        separated,
                        "{tool:?} option buttons overlap: {first:?} and {second:?}"
                    );
                }
            }
        }
    }
}
