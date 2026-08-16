use std::mem::size_of;

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetWindowRect, GetWindowTextW, IsIconic, IsWindowVisible,
};
use windows_sys::core::BOOL;

use crate::model::{DetectedTarget, PointI, RectI, TargetKind};
use crate::platform::WindowLocator;

pub struct Detector {
    windows: Vec<WindowRecord>,
    controls: Vec<WindowRecord>,
    controls_of: Option<HWND>,
}

#[derive(Debug, Clone, Copy)]
struct WindowRecord {
    hwnd: HWND,
    bounds: RectI,
}

struct WindowCollection {
    excluded: HWND,
    records: Vec<WindowRecord>,
}

impl Detector {
    pub fn new(excluded: HWND) -> Self {
        let mut collection = WindowCollection {
            excluded,
            records: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(collect_window),
                (&raw mut collection).cast::<WindowCollection>() as LPARAM,
            );
        }
        Self {
            windows: collection.records,
            controls: Vec::new(),
            controls_of: None,
        }
    }

    fn refresh_controls(&mut self, parent: HWND) {
        self.controls.clear();
        unsafe {
            EnumChildWindows(
                parent,
                Some(collect_control),
                (&raw mut self.controls).cast::<Vec<WindowRecord>>() as LPARAM,
            );
        }
        self.controls_of = Some(parent);
    }
}

impl WindowLocator for Detector {
    fn target_at(&mut self, point: PointI) -> Option<DetectedTarget> {
        let window = self
            .windows
            .iter()
            .find(|window| window.bounds.contains(point))
            .copied();
        let Some(window) = window else {
            self.controls.clear();
            self.controls_of = None;
            return None;
        };
        if self.controls_of != Some(window.hwnd) {
            self.refresh_controls(window.hwnd);
        }
        if let Some(control) = self
            .controls
            .iter()
            .filter(|control| control.bounds.contains(point))
            .min_by_key(|control| area(control.bounds))
        {
            return Some(DetectedTarget {
                bounds: control.bounds,
                kind: TargetKind::Control,
            });
        }
        Some(DetectedTarget {
            bounds: window.bounds,
            kind: TargetKind::Window,
        })
    }
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let collection = unsafe { &mut *(lparam as *mut WindowCollection) };
    if hwnd == collection.excluded
        || unsafe { IsWindowVisible(hwnd) } == 0
        || unsafe { IsIconic(hwnd) } != 0
        || is_cloaked(hwnd)
        || window_title_is_empty(hwnd)
    {
        return 1;
    }
    if let Some(bounds) = frame_bounds(hwnd) {
        collection.records.push(WindowRecord { hwnd, bounds });
    }
    1
}

unsafe extern "system" fn collect_control(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    let mut raw = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut raw) } != 0
        && let Some(bounds) = rect_from_raw(raw)
    {
        let records = unsafe { &mut *(lparam as *mut Vec<WindowRecord>) };
        records.push(WindowRecord { hwnd, bounds });
    }
    1
}

fn frame_bounds(hwnd: HWND) -> Option<RectI> {
    let mut raw = RECT::default();
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&raw mut raw).cast(),
            size_of::<RECT>() as u32,
        )
    };
    if result >= 0
        && let Some(bounds) = rect_from_raw(raw)
    {
        return Some(bounds);
    }
    (unsafe { GetWindowRect(hwnd, &mut raw) } != 0)
        .then(|| rect_from_raw(raw))
        .flatten()
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
    };
    result >= 0 && cloaked != 0
}

fn window_title_is_empty(hwnd: HWND) -> bool {
    let mut title = [0u16; 2];
    (unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) }) == 0
}

fn rect_from_raw(rect: RECT) -> Option<RectI> {
    (rect.right > rect.left && rect.bottom > rect.top).then(|| {
        RectI::new(
            rect.left,
            rect.top,
            (rect.right - rect.left) as u32,
            (rect.bottom - rect.top) as u32,
        )
    })
}

fn area(rect: RectI) -> u64 {
    u64::from(rect.width()) * u64::from(rect.height())
}
