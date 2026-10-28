//! Microphone permission: macOS TCC, or Windows' privacy registry settings.

use crate::Permission;

#[cfg(target_os = "macos")]
pub(crate) fn permission_status() -> Permission {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
        return Permission::Unknown;
    };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
    match status {
        AVAuthorizationStatus::Authorized => Permission::Granted,
        // Restricted: blocked by policy; the user can't grant it either.
        AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => Permission::Denied,
        AVAuthorizationStatus::NotDetermined => Permission::NotDetermined,
        _ => Permission::Unknown,
    }
}

/// Windows' three microphone privacy switches; any "Deny" makes WASAPI refuse
/// the stream. Windows never prompts a desktop app.
#[cfg(target_os = "windows")]
pub(crate) fn permission_status() -> Permission {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    const STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let switches = [
        // "Microphone access", for every user of the device.
        windows_consent(HKEY_LOCAL_MACHINE, STORE),
        // "Let apps access your microphone", for this user (desktop apps too).
        windows_consent(HKEY_CURRENT_USER, STORE),
        // "Let desktop apps access your microphone", for this user.
        windows_consent(HKEY_CURRENT_USER, &format!(r"{STORE}\NonPackaged")),
    ];
    if switches.iter().any(|s| s.as_deref() == Some("Deny")) {
        Permission::Denied
    } else if switches.iter().all(|s| s.as_deref() == Some("Allow")) {
        Permission::Granted
    } else {
        Permission::Unknown
    }
}

/// The consent store's "Value" under `subkey`, if readable.
#[cfg(target_os = "windows")]
fn windows_consent(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> Option<String> {
    use windows::{
        Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW},
        core::{HSTRING, w},
    };

    let subkey = HSTRING::from(subkey);
    // "Allow", "Deny", or "Prompt".
    let mut buffer = [0u16; 16];
    let mut bytes = size_of_val(&buffer) as u32;
    let status = unsafe {
        RegGetValueW(
            root,
            &subkey,
            w!("Value"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    status.ok().ok()?;
    // `bytes` includes the terminating NUL.
    let len = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn permission_status() -> Permission {
    Permission::Unknown
}
