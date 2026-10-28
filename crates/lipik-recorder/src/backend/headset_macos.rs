//! Finds a Bluetooth headset's output device on macOS, for `take_headset`.
//! Playing output moves a headset to this Mac; recording from its mic alone
//! does not.

use std::{
    ffi::c_void,
    ptr::{NonNull, null},
};

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertyScope, AudioObjectPropertySelector,
    kAudioDevicePropertyDeviceUID, kAudioDevicePropertyRelatedDevices, kAudioDevicePropertyStreams,
    kAudioDevicePropertyTransportType, kAudioDeviceTransportTypeBluetooth,
    kAudioDeviceTransportTypeBluetoothLE, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};
use objc2_core_foundation::{CFRetained, CFString};

/// The UID of the output device belonging to the Bluetooth headset whose
/// microphone has UID `input_uid`. `None` if that device is not Bluetooth or
/// has no related output device.
pub(crate) fn bluetooth_output_uid(input_uid: &str) -> Option<String> {
    let devices = devices();
    let Some(input) = devices
        .iter()
        .copied()
        .find(|&d| uid(d).as_deref() == Some(input_uid))
    else {
        log::debug!("take_headset: no CoreAudio device has UID {input_uid:?}");
        return None;
    };
    if !is_bluetooth(input) {
        log::debug!("take_headset: {input_uid:?} is not a Bluetooth device");
        return None;
    }
    let has_output = |d: AudioObjectID| {
        d != input
            && is_bluetooth(d)
            && !ids(
                d,
                kAudioDevicePropertyStreams,
                kAudioObjectPropertyScopeOutput,
            )
            .is_empty()
    };
    // The documented link.
    let related = ids(
        input,
        kAudioDevicePropertyRelatedDevices,
        kAudioObjectPropertyScopeGlobal,
    );
    if let Some(output) = related
        .iter()
        .copied()
        .find(|&d| has_output(d))
        .and_then(uid)
    {
        return Some(output);
    }
    // Fallback: both UIDs are the Bluetooth address plus ":input"/":output".
    let output = input_uid.rsplit_once(':').and_then(|(address, _)| {
        devices
            .iter()
            .copied()
            .filter(|&d| has_output(d))
            .filter_map(uid)
            .find(|u| u.rsplit_once(':').is_some_and(|(a, _)| a == address))
    });
    if output.is_none() {
        log::debug!("take_headset: found no output device for {input_uid:?}");
    }
    output
}

fn is_bluetooth(device: AudioObjectID) -> bool {
    scalar(device, kAudioDevicePropertyTransportType).is_some_and(|t| {
        t == kAudioDeviceTransportTypeBluetooth || t == kAudioDeviceTransportTypeBluetoothLE
    })
}

fn address(
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn devices() -> Vec<AudioObjectID> {
    ids(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )
}

/// A property holding an array of object IDs.
fn ids(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> Vec<AudioObjectID> {
    let address = address(selector, scope);
    let mut size = 0u32;
    // SAFETY: the address and size outlive the call.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || size == 0 {
        return Vec::new();
    }
    let mut out = vec![0 as AudioObjectID; size as usize / size_of::<AudioObjectID>()];
    let mut size = (out.len() * size_of::<AudioObjectID>()) as u32;
    // SAFETY: `out` has room for `size` bytes, and CoreAudio writes at most
    // that many, updating `size` to what it wrote.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::new(out.as_mut_ptr()).unwrap().cast::<c_void>(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    out.truncate(size as usize / size_of::<AudioObjectID>());
    out
}

fn scalar(object: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<u32> {
    let address = address(selector, kAudioObjectPropertyScopeGlobal);
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` has room for the `size` bytes CoreAudio may write.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    (status == 0 && size == size_of::<u32>() as u32).then_some(value)
}

fn uid(device: AudioObjectID) -> Option<String> {
    let address = address(
        kAudioDevicePropertyDeviceUID,
        kAudioObjectPropertyScopeGlobal,
    );
    let mut uid: *mut CFString = std::ptr::null_mut();
    let mut size = size_of::<*mut CFString>() as u32;
    // SAFETY: the UID property is a CFString pointer, returned under the
    // create rule (+1), which `CFRetained::from_raw` takes over.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut size),
            NonNull::from(&mut uid).cast(),
        )
    };
    if status != 0 {
        return None;
    }
    let uid = NonNull::new(uid)?;
    // SAFETY: see above; the pointer is non-null and owned.
    Some(unsafe { CFRetained::from_raw(uid) }.to_string())
}
