use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Result, anyhow};
use slint::platform::Key;
use slint::{CloseRequestResponse, ComponentHandle, GraphicsAPI, RenderingState, SharedString};

use crate::model::preview::{PreviewMode, PreviewSession};
use crate::model::{EditorKey, Point, RgbaFrame, Tool};
use crate::platform::{ImageClipboard, ImageSaveDialog};
use crate::rendering::PreviewRenderer;
use crate::ui::PreviewWindow;

thread_local! {
    static OPEN_PREVIEWS: RefCell<Vec<PreviewWindow>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Clone, Copy)]
enum OutputRequest {
    Copy,
    Save,
}

pub(super) fn open(image: RgbaFrame, save_directory: Option<PathBuf>) -> Result<()> {
    open_window(image, None, save_directory)
}

pub(super) fn open_ocr(
    image: RgbaFrame,
    recognized_text: String,
    save_directory: Option<PathBuf>,
) -> Result<()> {
    open_window(image, Some(recognized_text), save_directory)
}

fn open_window(
    image: RgbaFrame,
    recognized_text: Option<String>,
    save_directory: Option<PathBuf>,
) -> Result<()> {
    let window = PreviewWindow::new()?;
    if let Some(text) = recognized_text {
        window.set_preview_title("截图预览".into());
        window.set_ocr_text(text.into());
        window.set_ocr_panel_visible(true);
    }
    let session = Rc::new(RefCell::new(PreviewSession::new(image)));
    let renderer = Rc::new(RefCell::new(None::<PreviewRenderer>));
    let pending_output = Rc::new(Cell::new(None::<OutputRequest>));

    install_rendering_notifier(
        &window,
        session.clone(),
        renderer,
        pending_output.clone(),
        save_directory,
    )?;
    bind_commands(&window, session.clone(), pending_output);
    sync_ui(&window, &session.borrow());
    window
        .window()
        .on_close_requested(|| CloseRequestResponse::HideWindow);
    window.show()?;
    window.window().request_redraw();
    OPEN_PREVIEWS.with(|previews| {
        let mut previews = previews.borrow_mut();
        previews.retain(|preview| preview.window().is_visible());
        previews.push(window);
    });
    Ok(())
}

fn install_rendering_notifier(
    window: &PreviewWindow,
    session: Rc<RefCell<PreviewSession>>,
    renderer: Rc<RefCell<Option<PreviewRenderer>>>,
    pending_output: Rc<Cell<Option<OutputRequest>>>,
    save_directory: Option<PathBuf>,
) -> Result<()> {
    let weak = window.as_weak();
    window
        .window()
        .set_rendering_notifier(move |state, graphics| match state {
            RenderingState::RenderingSetup => {
                let GraphicsAPI::NativeOpenGL { get_proc_address } = graphics else {
                    eprintln!("preview requires the native OpenGL Slint renderer");
                    return;
                };
                let created = unsafe {
                    PreviewRenderer::new(session.borrow().image(), |name| get_proc_address(name))
                };
                match created {
                    Ok(mut created) => {
                        for font in crate::platform::ui_font_paths() {
                            created.load_font(&font);
                        }
                        *renderer.borrow_mut() = Some(created);
                    }
                    Err(error) => eprintln!("create editable preview renderer failed: {error:#}"),
                }
            }
            RenderingState::BeforeRendering => {
                let Some(component) = weak.upgrade() else {
                    return;
                };
                let canvas_width = component.get_canvas_width().max(1.0);
                let canvas_height = component.get_canvas_height().max(1.0);
                let mut session = session.borrow_mut();
                session.set_canvas_size(canvas_width, canvas_height);
                let size = component.window().size();
                let scale_factor = component.window().scale_factor();
                let mut renderer = renderer.borrow_mut();
                let Some(renderer) = renderer.as_mut() else {
                    return;
                };
                renderer.render(
                    size.width,
                    size.height,
                    scale_factor,
                    Point::new(component.get_canvas_x(), component.get_canvas_y()),
                    &session,
                );

                if let Some(request) = pending_output.take() {
                    match renderer.export(&session) {
                        Ok(image) => dispatch_output(request, image, save_directory.clone()),
                        Err(error) => eprintln!("export editable preview failed: {error:#}"),
                    }
                }
                sync_ui(&component, &session);
            }
            RenderingState::RenderingTeardown => {
                renderer.borrow_mut().take();
            }
            _ => {}
        })
        .map_err(|error| anyhow!("install preview rendering notifier: {error}"))?;
    Ok(())
}

fn dispatch_output(request: OutputRequest, image: RgbaFrame, save_directory: Option<PathBuf>) {
    let result = slint::invoke_from_event_loop(move || {
        let backend = crate::platform::current();
        let result = match request {
            OutputRequest::Copy => backend.write_image(&image),
            OutputRequest::Save => backend
                .choose_image_target(save_directory.as_deref())
                .and_then(|target| match target {
                    Some(target) => super::output::save_image(&image, &target),
                    None => Ok(()),
                }),
        };
        if let Err(error) = result {
            eprintln!("preview output failed: {error:#}");
        }
    });
    if let Err(error) = result {
        eprintln!("queue preview output failed: {error}");
    }
}

fn bind_commands(
    window: &PreviewWindow,
    session: Rc<RefCell<PreviewSession>>,
    pending_output: Rc<Cell<Option<OutputRequest>>>,
) {
    let weak = window.as_weak();
    let state = session.clone();
    window.on_choose_tool(move |index| {
        let Some(tool) = tool_from_index(index) else {
            return;
        };
        mutate_and_redraw(&weak, &state, |session| session.set_tool(tool));
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_choose_pan(move || {
        mutate_and_redraw(&weak, &state, PreviewSession::set_pan_mode);
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_undo(move || {
        mutate_and_redraw(&weak, &state, |session| session.key(EditorKey::Undo));
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_redo(move || {
        mutate_and_redraw(&weak, &state, |session| session.key(EditorKey::Redo));
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_zoom_in(move || {
        mutate_and_redraw(&weak, &state, |session| session.zoom_by(1.25));
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_zoom_out(move || {
        mutate_and_redraw(&weak, &state, |session| session.zoom_by(0.8));
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_actual_size(move || {
        mutate_and_redraw(&weak, &state, PreviewSession::actual_size);
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_fit(move || {
        mutate_and_redraw(&weak, &state, PreviewSession::fit_to_canvas);
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_rotate(move || {
        mutate_and_redraw(&weak, &state, PreviewSession::rotate_clockwise);
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_canvas_press(move |x, y| {
        mutate_and_redraw(&weak, &state, |session| {
            session.pointer_down(Point::new(x, y))
        });
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_canvas_double_click(move |x, y| {
        mutate_and_redraw(&weak, &state, |session| {
            session.double_click(Point::new(x, y))
        });
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_canvas_move(move |x, y| {
        mutate_and_redraw(&weak, &state, |session| {
            session.pointer_move(Point::new(x, y))
        });
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_canvas_release(move || {
        mutate_and_redraw(&weak, &state, PreviewSession::pointer_up);
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_canvas_scroll(move |x, y, delta| {
        mutate_and_redraw(&weak, &state, |session| {
            session.zoom_at(Point::new(x, y), delta)
        });
    });

    let weak = window.as_weak();
    let state = session.clone();
    window.on_key_input(move |text, control, shift, alt| {
        let changed = handle_key(&mut state.borrow_mut(), &text, control, shift, alt);
        if changed && let Some(window) = weak.upgrade() {
            sync_ui(&window, &state.borrow());
            window.window().request_redraw();
        }
        changed
    });

    let weak = window.as_weak();
    let pending = pending_output.clone();
    window.on_copy(move || {
        pending.set(Some(OutputRequest::Copy));
        if let Some(window) = weak.upgrade() {
            window.window().request_redraw();
        }
    });

    let weak = window.as_weak();
    window.on_save(move || {
        pending_output.set(Some(OutputRequest::Save));
        if let Some(window) = weak.upgrade() {
            window.window().request_redraw();
        }
    });

    let weak = window.as_weak();
    window.on_minimize(move || {
        if let Some(window) = weak.upgrade() {
            window.window().set_minimized(true);
        }
    });

    let weak = window.as_weak();
    window.on_toggle_maximize(move || {
        if let Some(window) = weak.upgrade() {
            let maximized = window.window().is_maximized();
            window.window().set_maximized(!maximized);
        }
    });

    let weak = window.as_weak();
    window.on_request_close(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
    });
}

fn mutate_and_redraw(
    weak: &slint::Weak<PreviewWindow>,
    state: &Rc<RefCell<PreviewSession>>,
    mutate: impl FnOnce(&mut PreviewSession) -> bool,
) {
    if mutate(&mut state.borrow_mut())
        && let Some(window) = weak.upgrade()
    {
        sync_ui(&window, &state.borrow());
        window.window().request_redraw();
    }
}

fn sync_ui(window: &PreviewWindow, session: &PreviewSession) {
    window.set_active_tool(tool_index(session.editor().tool()));
    window.set_pan_active(session.mode() == PreviewMode::Pan);
    window.set_can_undo(session.editor().can_undo());
    window.set_can_redo(session.editor().can_redo());
    window.set_zoom_percent((session.view().zoom() * 100.0).round() as i32);
}

fn handle_key(
    session: &mut PreviewSession,
    text: &SharedString,
    control: bool,
    shift: bool,
    alt: bool,
) -> bool {
    let value = text.as_str();
    if control && value.eq_ignore_ascii_case("z") {
        return session.key(if shift {
            EditorKey::Redo
        } else {
            EditorKey::Undo
        });
    }
    if control && value.eq_ignore_ascii_case("y") {
        return session.key(EditorKey::Redo);
    }
    let mapped = [
        (Key::Escape, EditorKey::Escape),
        (Key::Return, EditorKey::Enter),
        (Key::Backspace, EditorKey::Backspace),
        (Key::Delete, EditorKey::Delete),
        (Key::LeftArrow, EditorKey::Left),
        (Key::RightArrow, EditorKey::Right),
        (Key::UpArrow, EditorKey::Up),
        (Key::DownArrow, EditorKey::Down),
        (Key::Home, EditorKey::Home),
        (Key::End, EditorKey::End),
    ];
    if let Some((_, key)) = mapped
        .into_iter()
        .find(|(candidate, _)| SharedString::from(*candidate).as_str() == value)
    {
        return session.key(key);
    }
    if control || alt {
        return false;
    }
    let mut characters = value.chars();
    let Some(character) = characters.next() else {
        return false;
    };
    characters.next().is_none() && session.insert_character(character)
}

fn tool_from_index(index: i32) -> Option<Tool> {
    Some(match index {
        0 => Tool::Select,
        1 => Tool::Rectangle,
        2 => Tool::Circle,
        3 => Tool::Emotion,
        4 => Tool::Arrow,
        5 => Tool::Pen,
        6 => Tool::Mosaic,
        7 => Tool::Text,
        _ => return None,
    })
}

fn tool_index(tool: Tool) -> i32 {
    match tool {
        Tool::Select => 0,
        Tool::Rectangle => 1,
        Tool::Circle => 2,
        Tool::Emotion => 3,
        Tool::Arrow => 4,
        Tool::Pen => 5,
        Tool::Mosaic => 6,
        Tool::Text => 7,
    }
}
