use std::ptr::null_mut;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    HCURSOR, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_NO, IDC_SIZEALL, IDC_SIZENESW,
    IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, LoadCursorW, SetCursor,
};

use crate::model::PointerCursor;
use crate::platform::NativeCursorHost;

pub struct Host {
    arrow: HCURSOR,
    crosshair: HCURSOR,
    hand: HCURSOR,
    ibeam: HCURSOR,
    move_: HCURSOR,
    north_south: HCURSOR,
    east_west: HCURSOR,
    north_east_south_west: HCURSOR,
    north_west_south_east: HCURSOR,
    not_allowed: HCURSOR,
}

impl Host {
    pub fn new() -> Self {
        Self {
            arrow: load(IDC_ARROW),
            crosshair: load(IDC_CROSS),
            hand: load(IDC_HAND),
            ibeam: load(IDC_IBEAM),
            move_: load(IDC_SIZEALL),
            north_south: load(IDC_SIZENS),
            east_west: load(IDC_SIZEWE),
            north_east_south_west: load(IDC_SIZENESW),
            north_west_south_east: load(IDC_SIZENWSE),
            not_allowed: load(IDC_NO),
        }
    }

    fn handle(&self, cursor: PointerCursor) -> HCURSOR {
        match cursor {
            PointerCursor::Arrow => self.arrow,
            PointerCursor::Crosshair => self.crosshair,
            PointerCursor::Hand => self.hand,
            PointerCursor::IBeam => self.ibeam,
            PointerCursor::Move => self.move_,
            PointerCursor::Grab | PointerCursor::Grabbing => self.move_,
            PointerCursor::ResizeNorthSouth => self.north_south,
            PointerCursor::ResizeEastWest => self.east_west,
            PointerCursor::ResizeNorthEastSouthWest => self.north_east_south_west,
            PointerCursor::ResizeNorthWestSouthEast => self.north_west_south_east,
            PointerCursor::NotAllowed => self.not_allowed,
            PointerCursor::Hidden => null_mut(),
        }
    }
}

impl NativeCursorHost for Host {
    fn set_cursor(&mut self, cursor: PointerCursor) {
        unsafe { SetCursor(self.handle(cursor)) };
    }
}

fn load(name: windows_sys::core::PCWSTR) -> HCURSOR {
    unsafe { LoadCursorW(null_mut(), name) }
}
