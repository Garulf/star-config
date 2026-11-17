//! DirectInput 8 joysticks, read the way Star Citizen numbers them.
//!
//! Devices whose interface path carries `IG_` belong to XInput and are left to the gamepad
//! backend (SDL does the same). Every device gets a non-exclusive background cooperative level
//! on a message-only window, so reads never depend on which window has focus.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};

use windows::Win32::Devices::HumanInterfaceDevice::{
    DI8DEVCLASS_GAMECTRL, DIDATAFORMAT, DIDEVICEINSTANCEW, DIDEVICEOBJECTINSTANCEW, DIDF_ABSAXIS,
    DIDFT_ANYINSTANCE, DIDFT_AXIS, DIDFT_BUTTON, DIDFT_POV, DIEDFL_ATTACHEDONLY, DIENUM_CONTINUE,
    DIERR_INPUTLOST, DIERR_NOTACQUIRED, DIJOYSTATE2, DIOBJECTDATAFORMAT, DIPH_BYOFFSET,
    DIPH_DEVICE, DIPROPGUIDANDPATH, DIPROPHEADER, DIPROPRANGE, DIRECTINPUT_VERSION,
    DISCL_BACKGROUND, DISCL_NONEXCLUSIVE, DirectInput8Create, GUID_POV, GUID_RxAxis, GUID_RyAxis,
    GUID_RzAxis, GUID_Slider, GUID_XAxis, GUID_YAxis, GUID_ZAxis, IDirectInput8W,
    IDirectInputDevice8W,
};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::{BOOL, GUID, IUnknown, Interface, PCWSTR, w};

use crate::dinput_map::{
    AXIS_RANGE, DIPROP_GUIDANDPATH_ID, DIPROP_RANGE_ID, StickState, from_wide, property_key,
    stick_raws, usb_id_from_guid_data1, xinput_owns,
};
use crate::{Backend, DeviceInfo, DeviceKey, Raw};

const DIDFT_OPTIONAL: u32 = 0x8000_0000;
const POV_COUNT: usize = 4;
const BUTTON_COUNT: usize = 128;

const AXIS_OFFSETS: [usize; 8] = [
    offset_of!(DIJOYSTATE2, lX),
    offset_of!(DIJOYSTATE2, lY),
    offset_of!(DIJOYSTATE2, lZ),
    offset_of!(DIJOYSTATE2, lRx),
    offset_of!(DIJOYSTATE2, lRy),
    offset_of!(DIJOYSTATE2, lRz),
    offset_of!(DIJOYSTATE2, rglSlider),
    offset_of!(DIJOYSTATE2, rglSlider) + size_of::<i32>(),
];

static AXIS_GUIDS: [GUID; 8] = [
    GUID_XAxis,
    GUID_YAxis,
    GUID_ZAxis,
    GUID_RxAxis,
    GUID_RyAxis,
    GUID_RzAxis,
    GUID_Slider,
    GUID_Slider,
];
static POV_GUID: GUID = GUID_POV;

fn dword(n: usize) -> u32 {
    u32::try_from(n).expect("DirectInput sizes fit in a DWORD")
}

const DIDOI_ASPECTPOSITION: u32 = 0x100;

fn object(guid: *const GUID, offset: usize, kind: u32, flags: u32) -> DIOBJECTDATAFORMAT {
    DIOBJECTDATAFORMAT {
        pguid: guid,
        dwOfs: dword(offset),
        dwType: kind | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
        dwFlags: flags,
    }
}

fn joystick2_objects() -> Vec<DIOBJECTDATAFORMAT> {
    let axes = AXIS_GUIDS
        .iter()
        .zip(AXIS_OFFSETS)
        .map(|(guid, offset)| object(guid, offset, DIDFT_AXIS, DIDOI_ASPECTPOSITION));
    let povs = (0..POV_COUNT).map(|i| {
        let offset = offset_of!(DIJOYSTATE2, rgdwPOV) + i * size_of::<u32>();
        object(&POV_GUID, offset, DIDFT_POV, 0)
    });
    let buttons = (0..BUTTON_COUNT).map(|i| {
        let offset = offset_of!(DIJOYSTATE2, rgbButtons) + i * size_of::<u8>();
        object(core::ptr::null(), offset, DIDFT_BUTTON, 0)
    });
    axes.chain(povs).chain(buttons).collect()
}

fn header<T>(object: usize, how: u32) -> DIPROPHEADER {
    DIPROPHEADER {
        dwSize: dword(size_of::<T>()),
        dwHeaderSize: dword(size_of::<DIPROPHEADER>()),
        dwObj: dword(object),
        dwHow: how,
    }
}

fn set_data_format(device: &IDirectInputDevice8W) -> windows::core::Result<()> {
    let mut objects = joystick2_objects();
    let mut format = DIDATAFORMAT {
        dwSize: dword(size_of::<DIDATAFORMAT>()),
        dwObjSize: dword(size_of::<DIOBJECTDATAFORMAT>()),
        dwFlags: DIDF_ABSAXIS,
        dwDataSize: dword(size_of::<DIJOYSTATE2>()),
        dwNumObjs: dword(objects.len()),
        rgodf: objects.as_mut_ptr(),
    };
    // SAFETY: `format` and the `objects` it points to (whose GUID pointers are statics or null)
    // stay alive for the whole call; DirectInput copies the format before returning.
    unsafe { device.SetDataFormat(&mut format) }
}

fn has_axis(device: &IDirectInputDevice8W, offset: usize) -> bool {
    // SAFETY: DIDEVICEOBJECTINSTANCEW is plain integers, GUIDs and arrays; all zeroes is valid.
    let mut info: DIDEVICEOBJECTINSTANCEW = unsafe { core::mem::zeroed() };
    info.dwSize = dword(size_of::<DIDEVICEOBJECTINSTANCEW>());
    // SAFETY: `info` is a writable DIDEVICEOBJECTINSTANCEW with its size field set.
    unsafe { device.GetObjectInfo(&mut info, dword(offset), DIPH_BYOFFSET) }.is_ok()
}

fn set_axis_range(device: &IDirectInputDevice8W, offset: usize) -> bool {
    let (min, max) = AXIS_RANGE;
    let mut range = DIPROPRANGE {
        diph: header::<DIPROPRANGE>(offset, DIPH_BYOFFSET),
        lMin: min,
        lMax: max,
    };
    // SAFETY: the property key is DIPROP_RANGE, whose payload is a DIPROPRANGE; `range` starts
    // with its header, carries the full struct size and lives across the call.
    unsafe { device.SetProperty(property_key(DIPROP_RANGE_ID), &mut range.diph) }.is_ok()
}

fn present_axes(device: &IDirectInputDevice8W) -> u8 {
    AXIS_OFFSETS
        .iter()
        .enumerate()
        .filter(|&(_, &offset)| has_axis(device, offset) && set_axis_range(device, offset))
        .fold(0, |bits, (i, _)| bits | (1 << i))
}

fn interface_path(device: &IDirectInputDevice8W) -> Option<String> {
    let mut property = DIPROPGUIDANDPATH {
        diph: header::<DIPROPGUIDANDPATH>(0, DIPH_DEVICE),
        guidClass: GUID::zeroed(),
        wszPath: [0; 260],
    };
    // SAFETY: the property key is DIPROP_GUIDANDPATH, whose payload is a DIPROPGUIDANDPATH;
    // `property` starts with its header, carries the full struct size and lives across the call.
    unsafe { device.GetProperty(property_key(DIPROP_GUIDANDPATH_ID), &mut property.diph) }
        .ok()
        .map(|()| from_wide(&property.wszPath))
}

fn stick_state(raw: &DIJOYSTATE2) -> StickState {
    StickState {
        axes: [
            raw.lX,
            raw.lY,
            raw.lZ,
            raw.lRx,
            raw.lRy,
            raw.lRz,
            raw.rglSlider[0],
            raw.rglSlider[1],
        ],
        povs: raw.rgdwPOV,
        buttons: raw.rgbButtons,
    }
}

fn device_state(device: &IDirectInputDevice8W) -> windows::core::Result<StickState> {
    // SAFETY: Poll and GetDeviceState run on an acquired device with the joystick2 format.
    // `raw` is a writable DIJOYSTATE2, all zeroes is valid for it, and the size passed is its own.
    unsafe {
        device.Poll()?;
        let mut raw: DIJOYSTATE2 = core::mem::zeroed();
        device.GetDeviceState(
            dword(size_of::<DIJOYSTATE2>()),
            (&raw mut raw).cast::<c_void>(),
        )?;
        Ok(stick_state(&raw))
    }
}

struct Stick {
    device: IDirectInputDevice8W,
    present_axes: u8,
    previous: Option<StickState>,
    gone: bool,
}

impl Stick {
    fn sample(&self) -> Result<StickState, ()> {
        match device_state(&self.device) {
            Ok(state) => Ok(state),
            Err(e) if e.code() == DIERR_INPUTLOST || e.code() == DIERR_NOTACQUIRED => {
                // SAFETY: the device's format and cooperative level were set before the first
                // Acquire and never change.
                unsafe { self.device.Acquire() }.map_err(|_| ())?;
                device_state(&self.device).map_err(|_| ())
            }
            Err(_) => Err(()),
        }
    }
}

unsafe extern "system" fn collect(instance: *mut DIDEVICEINSTANCEW, found: *mut c_void) -> BOOL {
    // SAFETY: `found` is the `&mut Vec<DIDEVICEINSTANCEW>` that `enumerate` passes to
    // EnumDevices, which is still borrowed for the call; `instance` is valid while we run.
    unsafe {
        if let (Some(instance), Some(found)) = (
            instance.as_ref(),
            found.cast::<Vec<DIDEVICEINSTANCEW>>().as_mut(),
        ) {
            found.push(*instance);
        }
    }
    BOOL(DIENUM_CONTINUE as i32)
}

fn enumerate(input: &IDirectInput8W) -> Vec<DIDEVICEINSTANCEW> {
    let mut found: Vec<DIDEVICEINSTANCEW> = Vec::new();
    // SAFETY: `collect` only pushes copies into `found`, which outlives the synchronous call.
    let listed = unsafe {
        input.EnumDevices(
            DI8DEVCLASS_GAMECTRL,
            Some(collect),
            (&raw mut found).cast::<c_void>(),
            DIEDFL_ATTACHEDONLY,
        )
    };
    if listed.is_err() {
        return Vec::new();
    }
    found
}

fn message_window(module: HINSTANCE) -> Option<HWND> {
    // SAFETY: STATIC is a predefined class, so no registration is needed; the window is
    // message-only and destroyed in `Drop` on the thread that created it.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module),
            None,
        )
    }
    .ok()
}

fn direct_input(module: HINSTANCE) -> Option<IDirectInput8W> {
    let mut raw: *mut c_void = core::ptr::null_mut();
    // SAFETY: `raw` receives an owned IDirectInput8W reference for the IID we pass, which
    // `from_raw` takes over exactly once.
    unsafe {
        DirectInput8Create(
            module,
            DIRECTINPUT_VERSION,
            &IDirectInput8W::IID,
            &mut raw,
            None::<&IUnknown>,
        )
        .ok()?;
        (!raw.is_null()).then(|| IDirectInput8W::from_raw(raw))
    }
}

enum Opened {
    Stick(Stick),
    OwnedByXInput,
}

fn open_stick(
    input: &IDirectInput8W,
    window: HWND,
    instance: &DIDEVICEINSTANCEW,
) -> Option<Opened> {
    let mut device: Option<IDirectInputDevice8W> = None;
    // SAFETY: `guidInstance` came from EnumDevices and `device` is a writable out slot.
    unsafe { input.CreateDevice(&instance.guidInstance, &mut device, None::<&IUnknown>) }.ok()?;
    let device = device?;
    if interface_path(&device).is_some_and(|path| xinput_owns(&path)) {
        return Some(Opened::OwnedByXInput);
    }
    set_data_format(&device).ok()?;
    // SAFETY: `window` is our live message-only window; flags are a valid non-exclusive pair.
    unsafe { device.SetCooperativeLevel(window, DISCL_BACKGROUND | DISCL_NONEXCLUSIVE) }.ok()?;
    let present_axes = present_axes(&device);
    // SAFETY: format and cooperative level are set, as Acquire requires.
    unsafe { device.Acquire() }.ok()?;
    Some(Opened::Stick(Stick {
        device,
        present_axes,
        previous: None,
        gone: false,
    }))
}

pub struct DInputBackend {
    infos: Vec<DeviceInfo>,
    sticks: Vec<Stick>,
    unreadable: usize,
    window: Option<HWND>,
    _input: Option<IDirectInput8W>,
}

impl DInputBackend {
    pub fn open() -> Self {
        let mut backend = Self {
            infos: Vec::new(),
            sticks: Vec::new(),
            unreadable: 0,
            window: None,
            _input: None,
        };
        // SAFETY: a null module name asks for the running executable, which outlives us.
        let Ok(module) = (unsafe { GetModuleHandleW(PCWSTR::null()) }) else {
            return backend;
        };
        let module = HINSTANCE::from(module);
        let Some(window) = message_window(module) else {
            return backend;
        };
        backend.window = Some(window);
        let Some(input) = direct_input(module) else {
            return backend;
        };
        for instance in enumerate(&input) {
            match open_stick(&input, window, &instance) {
                Some(Opened::Stick(stick)) => {
                    let (vendor, product) = usb_id_from_guid_data1(instance.guidProduct.data1);
                    backend.infos.push(DeviceInfo {
                        vendor,
                        product,
                        name: from_wide(&instance.tszProductName),
                    });
                    backend.sticks.push(stick);
                }
                Some(Opened::OwnedByXInput) => {}
                None => backend.unreadable += 1,
            }
        }
        backend._input = Some(input);
        backend
    }
}

impl Drop for DInputBackend {
    fn drop(&mut self) {
        for stick in &self.sticks {
            // SAFETY: unacquiring a device we acquired; failure leaves nothing to clean up.
            let _ = unsafe { stick.device.Unacquire() };
        }
        self.sticks.clear();
        if let Some(window) = self.window.take() {
            // SAFETY: `window` was created by `message_window` on this thread and not destroyed.
            let _ = unsafe { DestroyWindow(window) };
        }
    }
}

impl Backend for DInputBackend {
    fn devices(&self) -> &[DeviceInfo] {
        &self.infos
    }

    fn unreadable(&self) -> usize {
        self.unreadable
    }

    fn read(&mut self) -> Vec<(DeviceKey, Result<Vec<Raw>, ()>)> {
        let mut out = Vec::new();
        for (key, stick) in self.sticks.iter_mut().enumerate() {
            if stick.gone {
                continue;
            }
            match stick.sample() {
                Ok(now) => {
                    let raws = stick_raws(stick.previous.as_ref(), &now, stick.present_axes);
                    let changed = stick.previous != Some(now);
                    stick.previous = Some(now);
                    if changed {
                        out.push((key, Ok(raws)));
                    }
                }
                Err(()) => {
                    stick.gone = true;
                    out.push((key, Err(())));
                }
            }
        }
        out
    }
}
