use std::ffi::{OsStr, OsString};
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use anyhow::{Result, anyhow};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetSaveFileNameW, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
};

use crate::platform::{ImageFileFormat, ImageSaveTarget};

const FILE_BUFFER_LEN: usize = 32_768;

pub fn choose_image_target(initial_directory: Option<&Path>) -> Result<Option<ImageSaveTarget>> {
    let filter = wide("PNG Image (*.png)\0*.png\0JPEG Image (*.jpg;*.jpeg)\0*.jpg;*.jpeg\0\0");
    let title = wide("Save Capture\0");
    let initial_directory = initial_directory.map(|path| wide_os(path.as_os_str()));

    loop {
        let mut file = [0_u16; FILE_BUFFER_LEN];
        set_initial_name(&mut file, "capture");
        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            lpstrFilter: filter.as_ptr(),
            nFilterIndex: 1,
            lpstrFile: file.as_mut_ptr(),
            nMaxFile: file.len() as u32,
            lpstrTitle: title.as_ptr(),
            lpstrInitialDir: initial_directory
                .as_ref()
                .map_or(std::ptr::null(), |path| path.as_ptr()),
            Flags: OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
            ..Default::default()
        };
        if unsafe { GetSaveFileNameW(&mut dialog) } == 0 {
            let error = unsafe { CommDlgExtendedError() };
            if error == 0 {
                return Ok(None);
            }
            return Err(anyhow!(
                "GetSaveFileNameW failed with dialog error {error:#x}"
            ));
        }

        let length = file
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(file.len());
        let path = PathBuf::from(OsString::from_wide(&file[..length]));
        let selected = if dialog.nFilterIndex == 2 {
            ImageFileFormat::Jpeg
        } else {
            ImageFileFormat::Png
        };
        let target = normalize_target(path, selected);
        if !target.path.exists() || confirm_overwrite(&target.path) {
            return Ok(Some(target));
        }
    }
}

fn set_initial_name(buffer: &mut [u16], name: &str) {
    let encoded: Vec<_> = OsStr::new(name).encode_wide().collect();
    let length = encoded.len().min(buffer.len().saturating_sub(1));
    buffer[..length].copy_from_slice(&encoded[..length]);
    buffer[length] = 0;
}

fn normalize_target(mut path: PathBuf, selected: ImageFileFormat) -> ImageSaveTarget {
    let explicit = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .and_then(|extension| match extension.as_str() {
            "png" => Some(ImageFileFormat::Png),
            "jpg" | "jpeg" => Some(ImageFileFormat::Jpeg),
            _ => None,
        });
    let format = explicit.unwrap_or(selected);
    if explicit.is_none() {
        path.set_extension(match format {
            ImageFileFormat::Png => "png",
            ImageFileFormat::Jpeg => "jpg",
        });
    }
    ImageSaveTarget { path, format }
}

fn confirm_overwrite(path: &Path) -> bool {
    let message = wide(&format!(
        "{} already exists.\nDo you want to replace it?\0",
        path.display()
    ));
    let title = wide("Confirm Save As\0");
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDYES
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().collect()
}

fn wide_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognized_extension_overrides_selected_filter() {
        assert_eq!(
            normalize_target(PathBuf::from("capture.jpeg"), ImageFileFormat::Png),
            ImageSaveTarget {
                path: PathBuf::from("capture.jpeg"),
                format: ImageFileFormat::Jpeg,
            }
        );
    }

    #[test]
    fn missing_or_unknown_extension_uses_selected_filter() {
        assert_eq!(
            normalize_target(PathBuf::from("capture"), ImageFileFormat::Png),
            ImageSaveTarget {
                path: PathBuf::from("capture.png"),
                format: ImageFileFormat::Png,
            }
        );
        assert_eq!(
            normalize_target(PathBuf::from("capture.data"), ImageFileFormat::Jpeg),
            ImageSaveTarget {
                path: PathBuf::from("capture.jpg"),
                format: ImageFileFormat::Jpeg,
            }
        );
    }
}
