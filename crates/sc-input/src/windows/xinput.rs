use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};

use crate::xinput_map::{PadState, pad_raws};
use crate::{Backend, DeviceInfo, DeviceKey, Raw};

const USER_INDICES: u32 = 4;
const XBOX_VENDOR: u16 = 0x045E;

struct Pad {
    user_index: u32,
    previous: Option<PadState>,
    gone: bool,
}

pub struct XInputBackend {
    infos: Vec<DeviceInfo>,
    pads: Vec<Pad>,
}

fn poll(user_index: u32) -> Option<PadState> {
    let mut state = XINPUT_STATE::default();
    // SAFETY: `state` is a valid, writable XINPUT_STATE for the duration of the call.
    let code = unsafe { XInputGetState(user_index, &mut state) };
    (code == 0).then(|| {
        let pad = state.Gamepad;
        PadState {
            buttons: pad.wButtons.0,
            left_trigger: pad.bLeftTrigger,
            right_trigger: pad.bRightTrigger,
            thumb_lx: pad.sThumbLX,
            thumb_ly: pad.sThumbLY,
            thumb_rx: pad.sThumbRX,
            thumb_ry: pad.sThumbRY,
        }
    })
}

impl XInputBackend {
    pub fn open() -> Self {
        let mut infos = Vec::new();
        let mut pads = Vec::new();
        for user_index in 0..USER_INDICES {
            if poll(user_index).is_some() {
                infos.push(DeviceInfo {
                    vendor: XBOX_VENDOR,
                    product: 0,
                    name: format!("Xbox controller {}", user_index + 1),
                });
                pads.push(Pad {
                    user_index,
                    previous: None,
                    gone: false,
                });
            }
        }
        Self { infos, pads }
    }
}

impl Backend for XInputBackend {
    fn devices(&self) -> &[DeviceInfo] {
        &self.infos
    }

    fn unreadable(&self) -> usize {
        0
    }

    fn read(&mut self) -> Vec<(DeviceKey, Result<Vec<Raw>, ()>)> {
        let mut out = Vec::new();
        for (key, pad) in self.pads.iter_mut().enumerate() {
            if pad.gone {
                continue;
            }
            match poll(pad.user_index) {
                Some(now) => {
                    let raws = pad_raws(pad.previous, now);
                    let changed = pad.previous != Some(now);
                    pad.previous = Some(now);
                    if changed {
                        out.push((key, Ok(raws)));
                    }
                }
                None => {
                    pad.gone = true;
                    out.push((key, Err(())));
                }
            }
        }
        out
    }
}
