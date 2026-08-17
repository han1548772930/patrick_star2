use std::cell::RefCell;

use anyhow::{Context, Result};
use slint::{
    CloseRequestResponse, ComponentHandle, Image, LogicalSize, PhysicalPosition, Rgba8Pixel,
    SharedPixelBuffer,
};

use crate::model::{PointI, RgbaFrame};
use crate::platform::{
    WindowFrame, WindowFrameClientArea, WindowFrameConfig, WindowFrameEvent, WindowFrameHost,
};
use crate::ui::PinWindow;

use super::{dpi_scale_at, set_slint_window_topmost};

const TITLEBAR_HEIGHT: f32 = 30.0;
const MIN_WIDTH: f32 = 160.0;
const MIN_HEIGHT: f32 = 90.0;
const MAX_INITIAL_WIDTH: f32 = 1280.0;
const MAX_INITIAL_HEIGHT: f32 = 900.0;

thread_local! {
    static OPEN_PINS: RefCell<Vec<OpenPin>> = const { RefCell::new(Vec::new()) };
}

struct OpenPin {
    window: PinWindow,
    _frame: Box<dyn WindowFrame>,
}

pub fn show(image: RgbaFrame) -> Result<()> {
    let bounds = image.bounds();
    let center = PointI::new(
        bounds.left.saturating_add(bounds.width() as i32 / 2),
        bounds.top.saturating_add(bounds.height() as i32 / 2),
    );
    let scale = dpi_scale_at(center, 1.0).max(0.01);
    let width = (image.width() as f32 / scale).clamp(MIN_WIDTH, MAX_INITIAL_WIDTH);
    let height = (image.height() as f32 / scale + TITLEBAR_HEIGHT)
        .clamp(MIN_HEIGHT, MAX_INITIAL_HEIGHT);

    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        image.pixels(),
        image.width(),
        image.height(),
    );
    let window = PinWindow::new().context("create pinned image window")?;
    window.set_pinned_image(Image::from_rgba8(pixels));
    window.set_pin_active(true);
    window
        .window()
        .set_position(PhysicalPosition::new(bounds.left, bounds.top));
    window.window().set_size(LogicalSize::new(width, height));
    window
        .window()
        .on_close_requested(|| CloseRequestResponse::HideWindow);

    let weak = window.as_weak();
    window.on_pin_toggle_requested(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let active = !window.get_pin_active();
        match set_slint_window_topmost(window.window(), active) {
            Ok(()) => window.set_pin_active(active),
            Err(error) => eprintln!("change pinned window topmost state failed: {error:#}"),
        }
    });

    let weak = window.as_weak();
    let frame = crate::platform::current().attach_window_frame(
        window.window(),
        WindowFrameConfig {
            titlebar_height: TITLEBAR_HEIGHT,
            caption_button_width: 48.0,
            minimum_width: MIN_WIDTH,
            minimum_height: MIN_HEIGHT,
            rounded_corners: true,
            always_on_top: true,
            client_areas: vec![WindowFrameClientArea::left(8.0, 0.0, 30.0, 30.0)],
        },
        Box::new(move |event| match event {
            WindowFrameEvent::CaptionHoverChanged(button) => {
                if let Some(window) = weak.upgrade() {
                    window.set_caption_hover(crate::ui::caption_button_value(button));
                }
            }
            WindowFrameEvent::Failed(error) => {
                eprintln!("pinned window frame failed: {error}");
            }
            WindowFrameEvent::Installed | WindowFrameEvent::Detached => {}
        }),
    )?;

    window.show().context("show pinned image window")?;
    window.window().request_redraw();
    OPEN_PINS.with(|pins| {
        let mut pins = pins.borrow_mut();
        pins.retain(|pin| pin.window.window().is_visible());
        pins.push(OpenPin {
            window,
            _frame: frame,
        });
    });
    Ok(())
}
