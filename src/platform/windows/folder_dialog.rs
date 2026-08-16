use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING};

const ERROR_CANCELLED_HRESULT: HRESULT = HRESULT(0x8007_04C7_u32 as i32);

pub fn choose_directory(initial_directory: &Path) -> Result<Option<PathBuf>> {
    let initialized =
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.is_ok();
    let result = show_dialog(initial_directory);
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

fn show_dialog(initial_directory: &Path) -> Result<Option<PathBuf>> {
    let dialog: IFileOpenDialog = unsafe {
        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .context("create Windows folder picker")?
    };
    unsafe {
        dialog
            .SetOptions(FOS_PICKFOLDERS | FOS_PATHMUSTEXIST)
            .context("configure Windows folder picker")?;
        dialog.SetTitle(&HSTRING::from("选择保存目录"))?;
    }

    if initial_directory.is_dir() {
        let path = HSTRING::from(initial_directory.as_os_str());
        if let Ok(item) = unsafe { SHCreateItemFromParsingName::<_, _, IShellItem>(&path, None) } {
            let _ = unsafe { dialog.SetFolder(&item) };
        }
    }

    if let Err(error) = unsafe { dialog.Show(None) } {
        if error.code() == ERROR_CANCELLED_HRESULT {
            return Ok(None);
        }
        return Err(error).context("show Windows folder picker");
    }
    let item = unsafe { dialog.GetResult() }.context("read Windows folder picker result")?;
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }
        .context("read selected directory path")?;
    let result = unsafe { name.to_string() };
    unsafe { CoTaskMemFree(Some(name.as_ptr().cast())) };
    let result = result.context("decode selected directory path")?;
    Ok(Some(PathBuf::from(result)))
}
