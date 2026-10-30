//! System audio through a Core Audio process tap (macOS 14.2+), and the list of
//! processes using the microphone (macOS 14.0+). Both read only Core Audio
//! objects: no screen recording permission is involved.

use anyhow::{anyhow, bail, Result};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::AnyThread;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceIsStackedKey,
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceUID,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyProcessObjectList,
    kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyBundleID,
    kAudioProcessPropertyIsRunningInput, kAudioProcessPropertyPID, kAudioSubDeviceUIDKey,
    kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey, kAudioTapPropertyFormat,
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertySelector, CATapDescription,
    CATapMuteBehavior,
};
use objc2_core_audio_types::{
    kAudioFormatFlagIsNonInterleaved, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::ffi::{c_void, CStr};
use std::ptr::NonNull;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

extern "C" {
    // Declared here with an untyped dictionary so the toll-free bridged
    // NSDictionary can be passed without a Core Foundation crate.
    #[link_name = "AudioHardwareCreateAggregateDevice"]
    fn create_aggregate_device(description: *const c_void, out_device: *mut AudioObjectID) -> i32;
}

fn address(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn get_value<T: Copy>(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
    qualifier: Option<&[u8]>,
) -> Option<T> {
    let mut addr = address(selector);
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let (q_size, q_ptr) = qualifier.map_or((0, std::ptr::null()), |q| {
        (q.len() as u32, q.as_ptr() as *const c_void)
    });
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            q_size,
            q_ptr,
            NonNull::from(&mut size),
            NonNull::new(value.as_mut_ptr() as *mut c_void)?,
        )
    };
    (status == 0 && size as usize == std::mem::size_of::<T>())
        .then(|| unsafe { value.assume_init() })
}

fn get_ids(object: AudioObjectID, selector: AudioObjectPropertySelector) -> Vec<AudioObjectID> {
    let mut addr = address(selector);
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || size == 0 {
        return Vec::new();
    }
    let mut ids = vec![0 as AudioObjectID; size as usize / std::mem::size_of::<AudioObjectID>()];
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(ids.as_mut_ptr() as *mut c_void).unwrap(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    ids.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
    ids
}

/// A CFString property. The getter returns it retained, so ownership moves
/// into the `Retained`.
fn get_string(object: AudioObjectID, selector: AudioObjectPropertySelector) -> Option<String> {
    let ptr: *mut NSString = get_value::<*mut NSString>(object, selector, None)?;
    let s = unsafe { Retained::from_raw(ptr) }?;
    Some(s.to_string())
}

/// The UID of every audio device this process sees, its own private ones
/// included.
#[cfg(test)]
pub fn device_uids() -> Vec<String> {
    get_ids(
        kAudioObjectSystemObject as AudioObjectID,
        objc2_core_audio::kAudioHardwarePropertyDevices,
    )
    .into_iter()
    .filter_map(|d| get_string(d, kAudioDevicePropertyDeviceUID))
    .collect()
}

/// Whether any process is playing through or recording from the device.
#[cfg(test)]
pub fn device_running_somewhere(device: AudioObjectID) -> bool {
    get_value::<u32>(
        device,
        objc2_core_audio::kAudioDevicePropertyDeviceIsRunningSomewhere,
        None,
    )
    .is_some_and(|v| v != 0)
}

/// Whether a process other than this one is playing sound.
pub fn others_playing() -> bool {
    let own = std::process::id() as i32;
    get_ids(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyProcessObjectList,
    )
    .into_iter()
    .filter(|&id| get_value::<i32>(id, kAudioProcessPropertyPID, None) != Some(own))
    .any(|id| {
        get_value::<u32>(
            id,
            objc2_core_audio::kAudioProcessPropertyIsRunningOutput,
            None,
        )
        .is_some_and(|v| v != 0)
    })
}

pub struct MicUser {
    pub pid: i32,
    pub bundle_id: String,
}

/// Processes currently reading from any input device.
pub fn processes_using_microphone() -> Vec<MicUser> {
    get_ids(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyProcessObjectList,
    )
    .into_iter()
    .filter(|&id| {
        get_value::<u32>(id, kAudioProcessPropertyIsRunningInput, None).is_some_and(|v| v != 0)
    })
    .filter_map(|id| {
        Some(MicUser {
            pid: get_value::<i32>(id, kAudioProcessPropertyPID, None)?,
            bundle_id: get_string(id, kAudioProcessPropertyBundleID).unwrap_or_default(),
        })
    })
    .collect()
}

pub fn default_output_device() -> Option<AudioObjectID> {
    get_value::<AudioObjectID>(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyDefaultOutputDevice,
        None,
    )
}

pub fn process_taps_supported() -> bool {
    AnyClass::get(c"CATapDescription").is_some()
}

struct TapContext {
    tx: SyncSender<Vec<f32>>,
    /// The tap's buffers are the last ones in the aggregate's input; any
    /// before them belong to the output device (a headset's microphone).
    tap_buffers: usize,
}

unsafe extern "C-unwind" fn tap_io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    let ctx = &*(client as *const TapContext);
    let list = input.as_ref();
    let all = std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize);
    let buffers = &all[all.len().saturating_sub(ctx.tap_buffers)..];
    let mono = match buffers {
        [] => return 0,
        [one] => {
            let channels = one.mNumberChannels.max(1) as usize;
            let len = one.mDataByteSize as usize / 4;
            if one.mData.is_null() || len == 0 {
                return 0;
            }
            let data = std::slice::from_raw_parts(one.mData as *const f32, len);
            data.chunks_exact(channels)
                .map(|f| f.iter().sum::<f32>() / channels as f32)
                .collect::<Vec<f32>>()
        }
        many => {
            let len = many[0].mDataByteSize as usize / 4;
            if len == 0 || many.iter().any(|b| b.mData.is_null()) {
                return 0;
            }
            let mut mono = vec![0.0f32; len];
            for b in many {
                let data = std::slice::from_raw_parts(
                    b.mData as *const f32,
                    (b.mDataByteSize as usize / 4).min(len),
                );
                for (m, s) in mono.iter_mut().zip(data) {
                    *m += s / many.len() as f32;
                }
            }
            mono
        }
    };
    let _ = ctx.tx.try_send(mono);
    0
}

/// Everything the machine plays (except this app), mixed to mono at the
/// tap's own sample rate. Dropping it tears the tap down.
pub struct SystemTap {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    ctx: *mut TapContext,
    pub sample_rate: f64,
    /// The output device the tap was built on.
    pub output_device: AudioObjectID,
}

unsafe impl Send for SystemTap {}

fn ns(key: &CStr) -> Retained<NSString> {
    NSString::from_str(key.to_str().unwrap_or_default())
}

fn dict(entries: &[(&CStr, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = entries.iter().map(|(k, _)| ns(k)).collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<&AnyObject> = entries.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&key_refs, &values)
}

impl SystemTap {
    pub fn start(name: &str) -> Result<(Self, Receiver<Vec<f32>>)> {
        if !process_taps_supported() {
            bail!("System audio capture needs macOS 14.2 or later");
        }

        let own_pid = std::process::id() as i32;
        let own_object = get_value::<AudioObjectID>(
            kAudioObjectSystemObject as AudioObjectID,
            kAudioHardwarePropertyTranslatePIDToProcessObject,
            Some(&own_pid.to_ne_bytes()),
        )
        .filter(|&id| id != 0);
        let excluded: Vec<Retained<NSNumber>> = own_object
            .map(|id| NSNumber::new_u32(id))
            .into_iter()
            .collect();
        let excluded = NSArray::from_retained_slice(&excluded);

        let desc = unsafe {
            let desc = CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded,
            );
            desc.setName(&NSString::from_str(name));
            desc.setPrivate(true);
            desc.setMuteBehavior(CATapMuteBehavior::Unmuted);
            desc
        };

        let mut tap: AudioObjectID = 0;
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap) };
        if status != 0 || tap == 0 {
            bail!("Could not create the system audio tap (OSStatus {status})");
        }

        let mut this = Self {
            tap,
            aggregate: 0,
            proc_id: None,
            ctx: std::ptr::null_mut(),
            sample_rate: 0.0,
            output_device: 0,
        };

        let format = get_value::<AudioStreamBasicDescription>(tap, kAudioTapPropertyFormat, None)
            .ok_or_else(|| anyhow!("Could not read the system audio format"))?;
        this.sample_rate = format.mSampleRate;
        let tap_uid = unsafe { desc.UUID().UUIDString() };

        let output_device =
            default_output_device().ok_or_else(|| anyhow!("No audio output device"))?;
        this.output_device = output_device;
        let output_uid = get_string(output_device, kAudioDevicePropertyDeviceUID)
            .ok_or_else(|| anyhow!("Could not read the output device id"))?;
        let output_uid = NSString::from_str(&output_uid);

        let aggregate_uid = NSString::from_str(&format!("{name}-{}", tap_uid));
        let display_name = NSString::from_str(name);
        let one = NSNumber::new_i32(1);
        let zero = NSNumber::new_i32(0);
        let sub_device = dict(&[(kAudioSubDeviceUIDKey, &*output_uid as &AnyObject)]);
        let sub_tap = dict(&[
            (kAudioSubTapUIDKey, &*tap_uid as &AnyObject),
            (kAudioSubTapDriftCompensationKey, &*one as &AnyObject),
        ]);
        let sub_devices = NSArray::from_retained_slice(&[sub_device]);
        let sub_taps = NSArray::from_retained_slice(&[sub_tap]);
        let description = dict(&[
            (kAudioAggregateDeviceNameKey, &*display_name as &AnyObject),
            (kAudioAggregateDeviceUIDKey, &*aggregate_uid as &AnyObject),
            (
                kAudioAggregateDeviceMainSubDeviceKey,
                &*output_uid as &AnyObject,
            ),
            (kAudioAggregateDeviceIsPrivateKey, &*one as &AnyObject),
            (kAudioAggregateDeviceIsStackedKey, &*zero as &AnyObject),
            (kAudioAggregateDeviceTapAutoStartKey, &*one as &AnyObject),
            (
                kAudioAggregateDeviceSubDeviceListKey,
                &*sub_devices as &AnyObject,
            ),
            (kAudioAggregateDeviceTapListKey, &*sub_taps as &AnyObject),
        ]);

        let status = unsafe {
            create_aggregate_device(
                Retained::as_ptr(&description) as *const c_void,
                &mut this.aggregate,
            )
        };
        if status != 0 || this.aggregate == 0 {
            bail!("Could not create the system audio device (OSStatus {status})");
        }

        let (tx, rx) = sync_channel::<Vec<f32>>(512);
        let non_interleaved = format.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
        let tap_buffers = if non_interleaved {
            format.mChannelsPerFrame.max(1) as usize
        } else {
            1
        };
        this.ctx = Box::into_raw(Box::new(TapContext { tx, tap_buffers }));
        let status = unsafe {
            AudioDeviceCreateIOProcID(
                this.aggregate,
                Some(tap_io_proc),
                this.ctx as *mut c_void,
                NonNull::from(&mut this.proc_id),
            )
        };
        if status != 0 {
            bail!("Could not attach to the system audio device (OSStatus {status})");
        }
        let status = unsafe { AudioDeviceStart(this.aggregate, this.proc_id) };
        if status != 0 {
            bail!("Could not start system audio capture (OSStatus {status})");
        }
        Ok((this, rx))
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        unsafe {
            if self.aggregate != 0 {
                if self.proc_id.is_some() {
                    AudioDeviceStop(self.aggregate, self.proc_id);
                    AudioDeviceDestroyIOProcID(self.aggregate, self.proc_id);
                }
                AudioHardwareDestroyAggregateDevice(self.aggregate);
            }
            if self.tap != 0 {
                AudioHardwareDestroyProcessTap(self.tap);
            }
            if !self.ctx.is_null() {
                drop(Box::from_raw(self.ctx));
            }
        }
    }
}
