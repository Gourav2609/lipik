//! Core Foundation and Accessibility basics shared by the code that reads
//! other apps' UI: the pill's focus checks, the dictation app context and the
//! notetaker's huddle reader.

use std::ffi::{c_char, c_void, CString};

pub type CfTypeRef = *const c_void;
pub type AxError = i32;

pub const AX_ERROR_NO_VALUE: AxError = -25212;
const UTF8: u32 = 0x0800_0100;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCopyAttributeValue(
        element: CfTypeRef,
        attribute: CfTypeRef,
        value: *mut CfTypeRef,
    ) -> AxError;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: CfTypeRef,
        text: *const c_char,
        encoding: u32,
    ) -> CfTypeRef;
    fn CFStringGetLength(string: CfTypeRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(string: CfTypeRef, buffer: *mut c_char, size: isize, encoding: u32)
        -> u8;
    fn CFGetTypeID(value: CfTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFRetain(value: CfTypeRef) -> CfTypeRef;
    fn CFRelease(value: CfTypeRef);
}

/// An owned (+1) Core Foundation reference.
pub struct Cf(pub CfTypeRef);

impl Clone for Cf {
    fn clone(&self) -> Self {
        retain(self.0)
    }
}

/// An owned copy of a borrowed reference (e.g. an item of a CFArray).
pub fn retain(value: CfTypeRef) -> Cf {
    if value.is_null() {
        return Cf(value);
    }
    // SAFETY: value is a live CF reference; the copy owns its own +1.
    Cf(unsafe { CFRetain(value) })
}

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: every Cf holds a reference returned by a Create/Copy call.
            unsafe { CFRelease(self.0) };
        }
    }
}

pub fn cf_string(text: &str) -> Cf {
    let text = CString::new(text).unwrap_or_default();
    // SAFETY: text is a valid NUL-terminated UTF-8 string.
    Cf(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) })
}

pub fn copy(element: &Cf, attribute: &str) -> Result<Cf, AxError> {
    let name = cf_string(attribute);
    let mut value: CfTypeRef = std::ptr::null();
    // SAFETY: element and name are live references; value receives a +1 reference.
    let error = unsafe { AXUIElementCopyAttributeValue(element.0, name.0, &mut value) };
    match (error, value.is_null()) {
        (0, false) => Ok(Cf(value)),
        (0, true) => Err(AX_ERROR_NO_VALUE),
        _ => Err(error),
    }
}

pub fn string(value: &Cf) -> Option<String> {
    string_ref(value.0)
}

/// A borrowed CF value as a String, when it is one.
pub fn string_ref(value: CfTypeRef) -> Option<String> {
    // SAFETY: value is a live CF reference, read as a string only after its
    // type is checked; the buffer holds the whole string and its NUL.
    unsafe {
        if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
            return None;
        }
        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), UTF8) + 1;
        let mut buffer = vec![0u8; size.max(1) as usize];
        if CFStringGetCString(value, buffer.as_mut_ptr().cast(), size, UTF8) == 0 {
            return None;
        }
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        buffer.truncate(end);
        String::from_utf8(buffer).ok()
    }
}
