// Take a look at the license at the top of the repository in the LICENSE file.

use std::path::PathBuf;

use crate::{translate::*, GString, StrV};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OSType {
    #[doc(alias = "G_WIN32_OS_ANY")]
    Any,
    #[doc(alias = "G_WIN32_OS_WORKSTATION")]
    Workstation,
    #[doc(alias = "G_WIN32_OS_SERVER")]
    Server,
}

#[doc(hidden)]
impl IntoGlib for OSType {
    type GlibType = ffi::GWin32OSType;

    #[inline]
    fn into_glib(self) -> Self::GlibType {
        match self {
            Self::Any => ffi::G_WIN32_OS_ANY,
            Self::Workstation => ffi::G_WIN32_OS_WORKSTATION,
            Self::Server => ffi::G_WIN32_OS_SERVER,
        }
    }
}

#[doc(alias = "g_win32_check_windows_version")]
pub fn check_windows_version(major: i32, minor: i32, spver: i32, os_type: OSType) -> bool {
    unsafe {
        from_glib(ffi::g_win32_check_windows_version(
            major,
            minor,
            spver,
            os_type.into_glib(),
        ))
    }
}

#[doc(alias = "g_win32_get_command_line")]
#[doc(alias = "get_command_line")]
pub fn command_line() -> StrV {
    unsafe { FromGlibPtrContainer::from_glib_full(ffi::g_win32_get_command_line()) }
}

#[doc(alias = "g_win32_error_message")]
pub fn error_message(error: i32) -> GString {
    unsafe { from_glib_full(ffi::g_win32_error_message(error)) }
}

#[doc(alias = "g_win32_getlocale")]
pub fn getlocale() -> GString {
    unsafe { from_glib_full(ffi::g_win32_getlocale()) }
}

#[doc(alias = "g_win32_get_package_installation_directory_of_module")]
#[doc(alias = "get_package_installation_directory_of_module")]
pub fn package_installation_directory_of_module(
    hmodule: std::os::windows::raw::HANDLE,
) -> Result<PathBuf, std::io::Error> {
    // # Safety
    // The underlying `GetModuleFilenameW` function has three possible
    // outcomes when a raw pointer get passed to it:
    // - When the pointer is a valid HINSTANCE of a DLL (e.g. acquired
    // through the `GetModuleHandleW`), it sets a file path to the
    // assigned "out" buffer and sets the return value to be the length
    // of said path string
    // - When the pointer is null, it sets the full path of the process'
    // executable binary to the assigned buffer and sets the return value
    // to be the length of said string
    // - Whenever the provided buffer size is too small, it will set a
    // truncated version of the path and return the length of said string
    // while also setting the thread-local last-error code to
    // `ERROR_INSUFFICIENT_BUFFER` (evaluates to 0x7A)
    // - When the pointer is not a valid HINSTANCE that isn't NULL (e.g.
    // a pointer to some GKeyFile), it will return 0 and set the last-error
    // code to `ERROR_MOD_NOT_FOUND` (evaluates to 0x7E)
    //
    // The `g_win32_get_package_installation_directory_of_module` already
    // handles all of the outcomes gracefully by:
    // - Preallocating a MAX_PATH-long array of wchar_t for the out buffer,
    // so that outcome #3 can be safely assumed to never happen
    // - Returning NULL when outcome #4 happens
    match unsafe {
        from_glib_full::<_, Option<PathBuf>>(
            ffi::g_win32_get_package_installation_directory_of_module(hmodule),
        )
    } {
        Some(pb) => Ok(pb),
        None => Err(std::io::Error::last_os_error()),
    }
}
