mod output;
mod preview;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::model::{CaptureIntent, CaptureOutcome, DesktopFrame, OverlayFeatures, RgbaFrame};
use crate::ocr::TextRecognizer;
use crate::platform::{
    Availability, CaptureOverlayHandoff, CaptureOverlayResult, DirectoryPicker,
    GlobalShortcutHost, GlobalShortcutRegistration, ImageClipboard, ImageSaveDialog,
    PlatformBackend, PlatformCapabilities, ScrollCaptureEvent, ScrollCaptureIntent,
    ScrollCaptureSource, Shortcut, SingleInstanceHost, TextClipboard,
};
#[cfg(feature = "opencv-orb")]
use crate::scroll::{OpenCvOrbMatcher, PushOutcome, ScrollConfig, ScrollSession};
use crate::settings::{Settings, SettingsStore};
use crate::ui::{AppTray, OcrLanguageChoice, SettingsDialog};
use anyhow::Context;

pub fn run() -> anyhow::Result<()> {
    let backend = crate::platform::current();
    let Some(_instance_guard) = backend.acquire_single_instance()? else {
        return Ok(());
    };
    let capabilities = backend.capabilities();
    anyhow::ensure!(
        capabilities.tray != Availability::Unavailable,
        "the system tray is unavailable on this platform session"
    );
    anyhow::ensure!(
        capabilities.global_shortcut != Availability::Unavailable,
        "global shortcuts are unavailable on this platform session"
    );

    let shortcut_host: Rc<dyn GlobalShortcutHost> =
        Rc::from(crate::platform::install_ui_platform()?);
    let settings_store = SettingsStore::for_current_user()?;
    let initial_settings = settings_store.load().unwrap_or_else(|error| {
        eprintln!("load settings failed, using defaults: {error:#}");
        Settings::default()
    });
    let settings = Rc::new(RefCell::new(initial_settings));
    let tray = AppTray::new()?;
    let capture_active = Arc::new(AtomicBool::new(false));
    let capture_settings = settings.clone();
    let worker_active = capture_active.clone();
    let capture: Rc<dyn Fn()> = Rc::new(move || {
        if worker_active.swap(true, Ordering::AcqRel) {
            return;
        }
        let settings = capture_settings.borrow().clone();
        match capture_once(&crate::platform::current(), &settings) {
            Ok(Some(task)) => {
                if let Err(error) = start_deferred_capture_worker(task, worker_active.clone()) {
                    worker_active.store(false, Ordering::Release);
                    eprintln!("start deferred capture task failed: {error:#}");
                }
            }
            Ok(None) => worker_active.store(false, Ordering::Release),
            Err(error) => {
                worker_active.store(false, Ordering::Release);
                eprintln!("capture failed: {error:#}");
            }
        }
    });

    let tray_capture = capture.clone();
    tray.on_capture(move || tray_capture());
    tray.on_quit(|| {
        if let Err(error) = crate::platform::current().cancel_scroll_capture() {
            eprintln!("cancel scroll capture during exit failed: {error:#}");
        }
        if let Err(error) = slint::quit_event_loop() {
            eprintln!("failed to quit event loop: {error}");
        }
    });

    let shortcut_registration = Rc::new(RefCell::new(Some(register_capture_shortcut(
        shortcut_host.as_ref(),
        settings.borrow().capture_shortcut,
        capture.clone(),
    )?)));

    let dialog_settings = settings.clone();
    let dialog_store = settings_store.clone();
    let dialog_host = shortcut_host.clone();
    let dialog_registration = shortcut_registration.clone();
    let dialog_capture = capture.clone();
    let settings_dialog = Rc::new(RefCell::new(None));
    let dialog_slot = settings_dialog.clone();
    tray.on_settings(move || {
        if let Err(error) = show_settings(
            dialog_slot.clone(),
            dialog_settings.clone(),
            dialog_store.clone(),
            dialog_host.clone(),
            dialog_registration.clone(),
            dialog_capture.clone(),
        ) {
            eprintln!("open settings failed: {error:#}");
        }
    });

    tray.show()?;
    slint::run_event_loop()?;
    Ok(())
}

fn capture_once(
    backend: &impl PlatformBackend,
    settings: &Settings,
) -> anyhow::Result<Option<DeferredCaptureTask>> {
    let capabilities = backend.capabilities();
    anyhow::ensure!(
        capabilities.desktop_capture != Availability::Unavailable,
        "desktop capture is unavailable on this platform session"
    );
    let frame = backend.capture_virtual_desktop()?;
    let features = overlay_features(capabilities);
    let CaptureOverlayResult { outcome, handoff } =
        backend.run_capture_overlay(frame, features)?;
    let CaptureOutcome::Confirmed {
        image,
        intent,
        desktop,
    } = outcome
    else {
        return Ok(None);
    };
    match intent {
        CaptureIntent::Clipboard => {
            anyhow::ensure!(
                capabilities.image_clipboard != Availability::Unavailable,
                "image clipboard is unavailable on this platform session"
            );
            backend.write_image(&image)?;
        }
        CaptureIntent::Save => {
            anyhow::ensure!(
                capabilities.image_save != Availability::Unavailable,
                "image save is unavailable on this platform session"
            );
            if let Some(target) =
                backend.choose_image_target(Some(settings.save_directory.as_path()))?
            {
                output::save_image(&image, &target)?;
            }
        }
        CaptureIntent::Pin => {
            anyhow::ensure!(
                capabilities.pinned_image != Availability::Unavailable,
                "pinned images are unavailable on this platform session"
            );
            backend.show_pinned_image(image)?;
        }
        CaptureIntent::ExtractText => {
            anyhow::ensure!(
                capabilities.text_recognition != Availability::Unavailable,
                "text recognition is unavailable on this platform session"
            );
            anyhow::ensure!(
                capabilities.text_clipboard != Availability::Unavailable,
                "text clipboard is unavailable on this platform session"
            );
            return Ok(Some(DeferredCaptureTask::Ocr {
                image,
                language_tag: settings.ocr_language.clone(),
                save_directory: settings.save_directory.clone(),
            }));
        }
        CaptureIntent::ScrollCapture => {
            return Ok(Some(DeferredCaptureTask::Scroll {
                task: ScrollCaptureTask {
                    initial: image,
                    desktop,
                },
                save_directory: settings.save_directory.clone(),
                handoff,
            }));
        }
    }
    Ok(None)
}

enum DeferredCaptureTask {
    Ocr {
        image: RgbaFrame,
        language_tag: Option<String>,
        save_directory: std::path::PathBuf,
    },
    Scroll {
        task: ScrollCaptureTask,
        save_directory: std::path::PathBuf,
        handoff: Option<Box<dyn CaptureOverlayHandoff>>,
    },
}

struct ScrollCaptureTask {
    initial: RgbaFrame,
    desktop: DesktopFrame,
}

struct ScrollCaptureOutput {
    image: RgbaFrame,
    intent: ScrollCaptureIntent,
}

fn start_deferred_capture_worker(
    task: DeferredCaptureTask,
    active: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    match task {
        DeferredCaptureTask::Ocr {
            image,
            language_tag,
            save_directory,
        } => start_ocr_capture_worker(image, language_tag, save_directory, active),
        DeferredCaptureTask::Scroll {
            task,
            save_directory,
            handoff,
        } => start_scroll_capture_worker(task, save_directory, active, handoff),
    }
}

fn start_ocr_capture_worker(
    image: RgbaFrame,
    language_tag: Option<String>,
    save_directory: std::path::PathBuf,
    active: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("patrick-star-ocr".into())
        .spawn(move || {
            let backend = crate::platform::current();
            let recognized = backend.recognize_text(&image, language_tag.as_deref());
            let text = match recognized {
                Ok(document) => {
                    let text = document.text();
                    if !text.trim().is_empty()
                        && let Err(error) = backend.write_text(&text)
                    {
                        eprintln!("copy OCR text failed: {error:#}");
                    }
                    if text.trim().is_empty() {
                        "未识别到文字内容。".to_owned()
                    } else {
                        text
                    }
                }
                Err(error) => format!("OCR 识别失败：{error:#}"),
            };
            active.store(false, Ordering::Release);
            if let Err(error) = slint::invoke_from_event_loop(move || {
                if let Err(error) = preview::open_ocr(image, text, Some(save_directory)) {
                    eprintln!("open OCR preview failed: {error:#}");
                }
            }) {
                eprintln!("dispatch OCR preview failed: {error}");
            }
        })
        .context("spawn OCR worker")?;
    Ok(())
}

fn start_scroll_capture_worker(
    task: ScrollCaptureTask,
    save_directory: std::path::PathBuf,
    active: Arc<AtomicBool>,
    handoff: Option<Box<dyn CaptureOverlayHandoff>>,
) -> anyhow::Result<()> {
    let (ready_sender, ready_receiver) = mpsc::sync_channel(0);
    std::thread::Builder::new()
        .name("patrick-star-scroll".into())
        .spawn(move || {
            let result = run_scroll_capture(
                &crate::platform::current(),
                &task.desktop,
                task.initial,
                || {
                    ready_sender
                        .send(())
                        .map_err(|_| anyhow::anyhow!("scroll overlay handoff receiver was dropped"))
                },
            );
            active.store(false, Ordering::Release);
            match result {
                Ok(Some(output)) => {
                    if let Err(error) = slint::invoke_from_event_loop(move || {
                        if let Err(error) = finish_scroll_capture(output, save_directory) {
                            eprintln!("finish scroll capture failed: {error:#}");
                        }
                    }) {
                        eprintln!("dispatch scroll capture preview failed: {error}");
                    }
                }
                Ok(None) => {}
                Err(error) => eprintln!("scroll capture failed: {error:#}"),
            }
        })
        .context("spawn scroll capture worker")?;
    ready_receiver
        .recv()
        .context("scroll capture stopped before its native windows were ready")?;
    drop(handoff);
    Ok(())
}

fn finish_scroll_capture(
    output: ScrollCaptureOutput,
    save_directory: std::path::PathBuf,
) -> anyhow::Result<()> {
    let backend = crate::platform::current();
    match output.intent {
        ScrollCaptureIntent::Edit => preview::open(output.image, Some(save_directory)),
        ScrollCaptureIntent::Save => {
            if let Some(target) = backend.choose_image_target(Some(save_directory.as_path()))? {
                output::save_image(&output.image, &target)?;
            }
            Ok(())
        }
        ScrollCaptureIntent::Clipboard => backend.write_image(&output.image),
    }
}

fn overlay_features(capabilities: crate::platform::Capabilities) -> OverlayFeatures {
    let available = |value| value != Availability::Unavailable;
    OverlayFeatures {
        extract_text: available(capabilities.text_recognition)
            && available(capabilities.text_clipboard),
        scroll_capture: cfg!(feature = "opencv-orb")
            && available(capabilities.scroll_capture_source)
            && available(capabilities.scroll_preview),
        // The language button is enabled only after its in-overlay state update
        // and persistence path are connected; an enabled no-op is not a capability.
        languages: false,
        save: available(capabilities.image_save),
        pin: available(capabilities.pinned_image),
    }
}

fn register_capture_shortcut(
    host: &dyn GlobalShortcutHost,
    shortcut: Shortcut,
    capture: Rc<dyn Fn()>,
) -> anyhow::Result<Box<dyn GlobalShortcutRegistration>> {
    host.register_global_shortcut(shortcut, Box::new(move || capture()))
}

fn show_settings(
    dialog_slot: Rc<RefCell<Option<SettingsDialog>>>,
    settings: Rc<RefCell<Settings>>,
    store: SettingsStore,
    shortcut_host: Rc<dyn GlobalShortcutHost>,
    registration: Rc<RefCell<Option<Box<dyn GlobalShortcutRegistration>>>>,
    capture: Rc<dyn Fn()>,
) -> anyhow::Result<()> {
    if let Some(dialog) = dialog_slot.borrow().as_ref() {
        return dialog.show(&settings.borrow());
    }
    let languages = crate::platform::current()
        .available_languages()
        .unwrap_or_else(|error| {
            eprintln!("enumerate OCR languages failed: {error:#}");
            Vec::new()
        })
        .into_iter()
        .map(|language| {
            let label = if language.native_name.trim().is_empty() {
                language.display_name
            } else {
                language.native_name
            };
            OcrLanguageChoice::new(language.tag, label)
        })
        .collect::<Vec<_>>();
    let dialog = SettingsDialog::new(&settings.borrow(), &languages)?;
    dialog.on_browse(move |initial| crate::platform::current().choose_directory(initial));
    let save_settings = settings.clone();
    dialog.on_save(move |next| {
        apply_settings(
            &save_settings,
            &store,
            shortcut_host.as_ref(),
            &registration,
            &capture,
            next,
        )
    });
    dialog.show(&settings.borrow())?;
    *dialog_slot.borrow_mut() = Some(dialog);
    Ok(())
}

fn apply_settings(
    settings: &Rc<RefCell<Settings>>,
    store: &SettingsStore,
    shortcut_host: &dyn GlobalShortcutHost,
    registration: &Rc<RefCell<Option<Box<dyn GlobalShortcutRegistration>>>>,
    capture: &Rc<dyn Fn()>,
    next: Settings,
) -> anyhow::Result<()> {
    next.validate()?;
    let previous = settings.borrow().clone();
    let rebind =
        previous.capture_shortcut != next.capture_shortcut || registration.borrow().is_none();
    let mut replacement = None;
    if rebind {
        registration.borrow_mut().take();
        match register_capture_shortcut(shortcut_host, next.capture_shortcut, capture.clone()) {
            Ok(value) => replacement = Some(value),
            Err(error) => {
                let rollback = register_capture_shortcut(
                    shortcut_host,
                    previous.capture_shortcut,
                    capture.clone(),
                )
                .map_err(|rollback| {
                    anyhow::anyhow!(
                        "new shortcut failed ({error:#}); restoring the previous shortcut also failed ({rollback:#})"
                    )
                })?;
                *registration.borrow_mut() = Some(rollback);
                return Err(error);
            }
        }
    }

    if let Err(error) = store.save(&next) {
        if rebind {
            drop(replacement.take());
            let rollback = register_capture_shortcut(
                shortcut_host,
                previous.capture_shortcut,
                capture.clone(),
            )?;
            *registration.borrow_mut() = Some(rollback);
        }
        return Err(error);
    }

    if rebind {
        *registration.borrow_mut() = replacement;
    }
    *settings.borrow_mut() = next;
    Ok(())
}

#[cfg(feature = "opencv-orb")]
fn run_scroll_capture(
    backend: &impl PlatformBackend,
    desktop: &DesktopFrame,
    initial: RgbaFrame,
    ready: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<Option<ScrollCaptureOutput>> {
    let capabilities = backend.capabilities();
    anyhow::ensure!(
        capabilities.scroll_capture_source != Availability::Unavailable,
        "scroll capture source is unavailable on this platform session"
    );
    anyhow::ensure!(
        capabilities.scroll_preview != Availability::Unavailable,
        "scroll preview is unavailable on this platform session"
    );

    let mut capture = backend.start_scroll_capture(initial.bounds())?;
    let mut preview = backend.open_scroll_preview(desktop, &initial)?;
    ready()?;
    let matcher = OpenCvOrbMatcher::new(1_200)?;
    let mut session = ScrollSession::new(initial, matcher, ScrollConfig::default());
    loop {
        match capture.next_event()? {
            ScrollCaptureEvent::Frame(frame) => match session.push(frame)? {
                PushOutcome::Appended { preview: dirty, .. } => {
                    let patch = session
                        .document()
                        .preview_patch(dirty)
                        .expect("scroll session returned a valid preview region");
                    preview.update(patch)?;
                }
                PushOutcome::Duplicate | PushOutcome::Rejected(_) => {}
            },
            ScrollCaptureEvent::Finished(intent) => {
                return Ok(Some(ScrollCaptureOutput {
                    image: session.finish(),
                    intent,
                }));
            }
            ScrollCaptureEvent::Cancelled => return Ok(None),
        }
    }
}

#[cfg(not(feature = "opencv-orb"))]
fn run_scroll_capture(
    _backend: &impl PlatformBackend,
    _desktop: &DesktopFrame,
    _initial: RgbaFrame,
    _ready: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<Option<ScrollCaptureOutput>> {
    anyhow::bail!("scroll capture requires the opencv-orb feature")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Availability, Capabilities};

    fn capabilities(value: Availability) -> Capabilities {
        Capabilities {
            desktop_capture: value,
            window_detection: value,
            image_clipboard: value,
            text_clipboard: value,
            image_save: value,
            pinned_image: value,
            text_recognition: value,
            scroll_capture_source: value,
            scroll_preview: value,
            global_shortcut: value,
            tray: value,
            capture_exclusion: value,
        }
    }

    #[test]
    fn overlay_commands_follow_composed_platform_capabilities() {
        let features = overlay_features(capabilities(Availability::Native));
        assert!(features.extract_text);
        assert_eq!(features.scroll_capture, cfg!(feature = "opencv-orb"));
        assert!(features.save);
        assert!(features.pin);
        assert!(!features.languages);
    }

    #[test]
    fn composed_commands_are_disabled_when_one_required_capability_is_missing() {
        let mut values = capabilities(Availability::Portal);
        values.text_clipboard = Availability::Unavailable;
        values.scroll_preview = Availability::Unavailable;
        values.image_save = Availability::Unavailable;
        values.pinned_image = Availability::Unavailable;
        let features = overlay_features(values);
        assert!(!features.extract_text);
        assert!(!features.scroll_capture);
        assert!(!features.save);
        assert!(!features.pin);
    }
}
