use std::ffi::{CStr, c_void};
use std::mem::size_of;
use std::ptr::null_mut;

use anyhow::{Result, anyhow};
use windows_sys::Win32::Foundation::{HWND, PROC};
use windows_sys::Win32::Graphics::Gdi::{GetDC, HDC, ReleaseDC};
use windows_sys::Win32::Graphics::OpenGL::{
    ChoosePixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_MAIN_PLANE,
    PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat, SwapBuffers,
    wglCreateContext, wglDeleteContext, wglGetCurrentContext, wglGetCurrentDC, wglGetProcAddress,
    wglMakeCurrent,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA};

const WGL_CONTEXT_MAJOR_VERSION_ARB: i32 = 0x2091;
const WGL_CONTEXT_MINOR_VERSION_ARB: i32 = 0x2092;
const WGL_CONTEXT_PROFILE_MASK_ARB: i32 = 0x9126;
const WGL_CONTEXT_CORE_PROFILE_BIT_ARB: i32 = 0x0000_0001;

type CreateContextAttribs = unsafe extern "system" fn(HDC, HGLRC, *const i32) -> HGLRC;
type SwapInterval = unsafe extern "system" fn(i32) -> i32;

pub struct Surface {
    hwnd: HWND,
    dc: HDC,
    context: HGLRC,
}

impl Surface {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let dc = unsafe { GetDC(hwnd) };
        anyhow::ensure!(!dc.is_null(), "GetDC(capture window) failed");
        let result = unsafe { create_context(hwnd, dc) };
        if result.is_err() {
            unsafe { ReleaseDC(hwnd, dc) };
        }
        result
    }

    pub fn proc_address(&self, name: &CStr) -> *const c_void {
        unsafe { load_proc(name) }
    }

    pub fn ensure_current(&self) -> Result<()> {
        if self.is_current() {
            return Ok(());
        }
        anyhow::ensure!(
            unsafe { wglMakeCurrent(self.dc, self.context) } != 0,
            "wglMakeCurrent failed"
        );
        Ok(())
    }

    pub fn is_current(&self) -> bool {
        (unsafe { wglGetCurrentContext() }) == self.context
            && (unsafe { wglGetCurrentDC() }) == self.dc
    }

    pub fn present(&self) -> Result<()> {
        anyhow::ensure!(unsafe { SwapBuffers(self.dc) } != 0, "SwapBuffers failed");
        Ok(())
    }
}

unsafe fn create_context(hwnd: HWND, dc: HDC) -> Result<Surface> {
    let descriptor = PIXELFORMATDESCRIPTOR {
        nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
        nVersion: 1,
        dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
        iPixelType: PFD_TYPE_RGBA,
        cColorBits: 32,
        cAlphaBits: 8,
        cDepthBits: 0,
        cStencilBits: 8,
        iLayerType: PFD_MAIN_PLANE as u8,
        ..Default::default()
    };
    let format = unsafe { ChoosePixelFormat(dc, &descriptor) };
    anyhow::ensure!(format != 0, "ChoosePixelFormat failed");
    anyhow::ensure!(
        unsafe { SetPixelFormat(dc, format, &descriptor) } != 0,
        "SetPixelFormat failed"
    );

    let legacy = unsafe { wglCreateContext(dc) };
    anyhow::ensure!(!legacy.is_null(), "wglCreateContext failed");
    if unsafe { wglMakeCurrent(dc, legacy) } == 0 {
        unsafe { wglDeleteContext(legacy) };
        return Err(anyhow!("wglMakeCurrent for bootstrap context failed"));
    }

    let create_ptr = unsafe { load_proc(c"wglCreateContextAttribsARB") };
    if create_ptr.is_null() {
        unsafe {
            wglMakeCurrent(null_mut(), null_mut());
            wglDeleteContext(legacy);
        }
        return Err(anyhow!("OpenGL 3.3 context creation is not supported"));
    }
    let create: CreateContextAttribs = unsafe { std::mem::transmute(create_ptr) };
    let attributes = [
        WGL_CONTEXT_MAJOR_VERSION_ARB,
        3,
        WGL_CONTEXT_MINOR_VERSION_ARB,
        3,
        WGL_CONTEXT_PROFILE_MASK_ARB,
        WGL_CONTEXT_CORE_PROFILE_BIT_ARB,
        0,
    ];
    let modern = unsafe { create(dc, null_mut(), attributes.as_ptr()) };
    if modern.is_null() {
        unsafe {
            wglMakeCurrent(null_mut(), null_mut());
            wglDeleteContext(legacy);
        }
        return Err(anyhow!("failed to create an OpenGL 3.3 core context"));
    }
    unsafe {
        wglMakeCurrent(null_mut(), null_mut());
        wglDeleteContext(legacy);
    }
    if unsafe { wglMakeCurrent(dc, modern) } == 0 {
        unsafe { wglDeleteContext(modern) };
        return Err(anyhow!("wglMakeCurrent for OpenGL 3.3 context failed"));
    }

    let swap_interval_ptr = unsafe { load_proc(c"wglSwapIntervalEXT") };
    if !swap_interval_ptr.is_null() {
        let swap_interval: SwapInterval = unsafe { std::mem::transmute(swap_interval_ptr) };
        unsafe { swap_interval(1) };
    }

    Ok(Surface {
        hwnd,
        dc,
        context: modern,
    })
}

unsafe fn load_proc(name: &CStr) -> *const c_void {
    let extension = unsafe { wglGetProcAddress(name.as_ptr().cast()) };
    let extension = proc_to_ptr(extension);
    let address = extension as usize;
    if !extension.is_null() && address > 3 && address != usize::MAX {
        return extension;
    }

    let mut module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr().cast()) };
    if module.is_null() {
        module = unsafe { LoadLibraryA(c"opengl32.dll".as_ptr().cast()) };
    }
    if module.is_null() {
        return std::ptr::null();
    }
    proc_to_ptr(unsafe { GetProcAddress(module, name.as_ptr().cast()) })
}

fn proc_to_ptr(proc: PROC) -> *const c_void {
    proc.map_or(std::ptr::null(), |function| {
        function as *const () as *const c_void
    })
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            wglMakeCurrent(null_mut(), null_mut());
            wglDeleteContext(self.context);
            ReleaseDC(self.hwnd, self.dc);
        }
    }
}
