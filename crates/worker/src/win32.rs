//! Direct Win32 calls.
//!
//! PowerShell reached these through .NET wrappers. Each one is isolated here so
//! the rest of the worker stays platform-neutral, and so the unsafe surface is
//! one file rather than scattered through the provisioning logic.
#![cfg(windows)]

use anyhow::{bail, Result};
use std::ffi::OsStr;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_SUCCESS, FALSE, HANDLE, HWND, LPARAM, MAX_PATH, TRUE, WPARAM,
};
use windows_sys::Win32::Security::*;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// The registry root to read from, so a raw handle never crosses this
/// module's boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

impl Hive {
    fn handle(self) -> HKEY {
        match self {
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::CurrentUser => HKEY_CURRENT_USER,
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(std::iter::once(0)).collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|value| *value == 0).unwrap_or(buffer.len());
    std::ffi::OsString::from_wide(&buffer[..end]).to_string_lossy().into_owned()
}

/// Read a string value, following the 32-bit view where the caller asked for it
/// through an explicit `WOW6432Node` path.
pub fn read_string(hive: Hive, key: &str, name: &str) -> Option<String> {
    unsafe {
        let mut handle: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(hive.handle(), wide(key).as_ptr(), 0, KEY_READ, &mut handle) != ERROR_SUCCESS {
            return None;
        }
        let mut kind = 0u32;
        let mut size = 0u32;
        let name = wide(name);
        let status =
            RegQueryValueExW(handle, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut size);
        if status != ERROR_SUCCESS || size == 0 {
            CloseHandle(handle as HANDLE);
            return None;
        }
        let mut buffer = vec![0u16; (size as usize / 2) + 1];
        let status = RegQueryValueExW(
            handle,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            buffer.as_mut_ptr() as *mut u8,
            &mut size,
        );
        RegCloseKey(handle);
        if status != ERROR_SUCCESS {
            return None;
        }
        Some(from_wide(&buffer))
    }
}

/// The product name as a person would recognise it.
///
/// The registry's `ProductName` still reads "Windows 10" on Windows 11, so the
/// build number decides. PowerShell reached the corrected name through WMI;
/// this avoids taking a WMI dependency for one display field.
pub fn product_name() -> Option<String> {
    const CURRENT_VERSION: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";
    let name = read_string(Hive::LocalMachine, CURRENT_VERSION, "ProductName")?;
    let build: u32 =
        read_string(Hive::LocalMachine, CURRENT_VERSION, "CurrentBuildNumber").and_then(|value| value.parse().ok()).unwrap_or(0);
    Some(if build >= 22000 { name.replace("Windows 10", "Windows 11") } else { name })
}

/// The machine's real architecture, read from the registry so an emulated
/// process does not report the architecture it is being emulated as.
pub fn machine_architecture() -> Option<String> {
    read_string(
        Hive::LocalMachine,
        "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
        "PROCESSOR_ARCHITECTURE",
    )
}

/// Write the user's PATH and tell running programs it changed. .NET broadcast
/// this automatically; a bare registry write would leave every open shell with
/// the old value until sign-out.
pub fn write_user_path(value: &str) -> Result<()> {
    unsafe {
        let mut handle: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(Hive::CurrentUser.handle(), wide("Environment").as_ptr(), 0, KEY_SET_VALUE, &mut handle) != ERROR_SUCCESS {
            bail!("사용자 환경 변수를 열지 못했어요.");
        }
        let data = wide(value);
        let status = RegSetValueExW(
            handle,
            wide("Path").as_ptr(),
            0,
            REG_EXPAND_SZ,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
        );
        RegCloseKey(handle);
        if status != ERROR_SUCCESS {
            bail!("사용자 PATH를 기록하지 못했어요.");
        }
        let mut result: usize = 0;
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0 as WPARAM,
            wide("Environment").as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
    Ok(())
}

/// Whether this process runs with administrator rights.
pub fn is_administrator() -> bool {
    unsafe {
        let mut sid = std::ptr::null_mut();
        let authority = SID_IDENTIFIER_AUTHORITY { Value: [0, 0, 0, 0, 0, 5] };
        if AllocateAndInitializeSid(
            &authority,
            2,
            0x00000020, // SECURITY_BUILTIN_DOMAIN_RID
            0x00000220, // DOMAIN_ALIAS_RID_ADMINS
            0,
            0,
            0,
            0,
            0,
            0,
            &mut sid,
        ) == FALSE
        {
            return false;
        }
        let mut member = FALSE;
        let checked = CheckTokenMembership(std::ptr::null_mut(), sid, &mut member);
        FreeSid(sid);
        checked != FALSE && member != FALSE
    }
}

/// Windows accepts either separator in a path, so two spellings of the same
/// file must compare equal.
fn normalize(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

/// The process id whose image is exactly this executable, if one is running.
pub fn process_with_image(executable: &Path) -> Option<u32> {
    let wanted = normalize(&executable.to_string_lossy());
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        if Process32FirstW(snapshot, &mut entry) != FALSE {
            loop {
                if let Some(path) = image_path(entry.th32ProcessID) {
                    if normalize(&path) == wanted {
                        found = Some(entry.th32ProcessID);
                        break;
                    }
                }
                if Process32NextW(snapshot, &mut entry) == FALSE {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        found
    }
}

fn image_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; MAX_PATH as usize * 2];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == FALSE {
            return None;
        }
        Some(from_wide(&buffer[..size as usize]))
    }
}

struct WindowSearch {
    pid: u32,
    handle: HWND,
}

/// The application's main window: the first visible, owner-less top-level
/// window belonging to the process, which is what .NET reported.
pub fn main_window(pid: u32) -> Option<(isize, String)> {
    unsafe {
        let mut search = WindowSearch { pid, handle: std::ptr::null_mut() };
        EnumWindows(Some(enumerate), &mut search as *mut WindowSearch as LPARAM);
        if search.handle.is_null() {
            return None;
        }
        let mut title = vec![0u16; 512];
        let length = GetWindowTextW(search.handle, title.as_mut_ptr(), title.len() as i32);
        let text = if length > 0 { from_wide(&title[..length as usize]) } else { String::new() };
        Some((search.handle as isize, text))
    }
}

unsafe extern "system" fn enumerate(window: HWND, parameter: LPARAM) -> i32 {
    let search = &mut *(parameter as *mut WindowSearch);
    let mut pid = 0u32;
    GetWindowThreadProcessId(window, &mut pid);
    if pid == search.pid && IsWindowVisible(window) != FALSE && GetWindow(window, GW_OWNER).is_null()
    {
        search.handle = window;
        return FALSE;
    }
    TRUE
}

/// Whether the window's thread is still pumping messages.
pub fn window_responding(handle: isize) -> bool {
    unsafe {
        let mut result: usize = 0;
        SendMessageTimeoutW(
            handle as HWND,
            WM_NULL,
            0 as WPARAM,
            0 as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        ) != 0
    }
}

/// Authenticode verification of a downloaded installer.
pub fn verify_authenticode(path: &Path) -> bool {
    unsafe { wintrust::verify(path) }
}

/// The signing certificate's subject, for the publisher check.
pub fn signer_subject(path: &Path) -> Option<String> {
    unsafe { wintrust::subject(path) }
}

mod wintrust {
    use super::{from_wide, wide};
    use std::path::Path;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::TRUE;
    use windows_sys::Win32::Security::Cryptography::*;
    use windows_sys::Win32::Security::WinTrust::*;

    const ACTION: GUID = GUID {
        data1: 0x00AAC56B,
        data2: 0xCD44,
        data3: 0x11D0,
        data4: [0x8C, 0xC2, 0x00, 0xC0, 0x4F, 0xC2, 0x95, 0xEE],
    };

    pub unsafe fn verify(path: &Path) -> bool {
        let file = wide(&path.to_string_lossy());
        let mut info: WINTRUST_FILE_INFO = std::mem::zeroed();
        info.cbStruct = std::mem::size_of::<WINTRUST_FILE_INFO>() as u32;
        info.pcwszFilePath = file.as_ptr();
        let mut data: WINTRUST_DATA = std::mem::zeroed();
        data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        data.dwUIChoice = WTD_UI_NONE;
        data.fdwRevocationChecks = WTD_REVOKE_NONE;
        data.dwUnionChoice = WTD_CHOICE_FILE;
        data.dwStateAction = WTD_STATEACTION_VERIFY;
        data.Anonymous.pFile = &mut info;
        let mut action = ACTION;
        let status = WinVerifyTrust(
            std::ptr::null_mut(),
            &mut action,
            &mut data as *mut WINTRUST_DATA as *mut std::ffi::c_void,
        );
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(
            std::ptr::null_mut(),
            &mut action,
            &mut data as *mut WINTRUST_DATA as *mut std::ffi::c_void,
        );
        status == 0
    }

    pub unsafe fn subject(path: &Path) -> Option<String> {
        let file = wide(&path.to_string_lossy());
        let mut store = std::ptr::null_mut();
        let mut message = std::ptr::null_mut();
        let ok = CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            file.as_ptr() as *const std::ffi::c_void,
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut store,
            &mut message,
            std::ptr::null_mut(),
        );
        if ok != TRUE || store.is_null() {
            return None;
        }
        let context = CertEnumCertificatesInStore(store, std::ptr::null_mut());
        let mut result = None;
        if !context.is_null() {
            let mut buffer = vec![0u16; 1024];
            let length = CertGetNameStringW(
                context,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                std::ptr::null_mut(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
            );
            if length > 1 {
                result = Some(from_wide(&buffer[..length as usize]));
            }
            CertFreeCertificateContext(context);
        }
        CertCloseStore(store, 0);
        if !message.is_null() {
            CryptMsgClose(message);
        }
        result
    }
}
