use anyhow::Result;
use serde::Serialize;
use std::{marker::PhantomData, rc::Rc};

/// Windows tracks execution requests per thread. This guard must never move to
/// another thread: acquire and Drop must run on the same server supervisor.
pub struct SleepGuard {
    _thread_bound: PhantomData<Rc<()>>,
}

impl SleepGuard {
    pub fn acquire() -> Result<Self> {
        #[cfg(windows)]
        {
            // Only system idle sleep: the display may still turn off normally.
            if unsafe {
                native::SetThreadExecutionState(native::CONTINUOUS | native::SYSTEM_REQUIRED)
            } == 0
            {
                anyhow::bail!("O Windows não aceitou o pedido para impedir suspensão automática");
            }
            Ok(Self {
                _thread_bound: PhantomData,
            })
        }
        #[cfg(not(windows))]
        anyhow::bail!("Proteção contra suspensão disponível apenas no Windows nesta versão")
    }
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: !Send/!Sync keeps this guard on the thread that acquired it.
        unsafe {
            native::SetThreadExecutionState(native::CONTINUOUS);
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Supply {
    pub source: &'static str,
    pub battery_percent: Option<u8>,
    pub has_battery: Option<bool>,
}

fn decode_supply(ac: u8, flags: u8, percent: u8) -> Supply {
    let has_battery = if flags == 255 {
        None
    } else {
        Some(flags & 128 == 0)
    };
    Supply {
        source: match ac {
            1 => "ac",
            0 if has_battery == Some(true) => "battery",
            _ => "unknown",
        },
        battery_percent: if has_battery == Some(true) && percent <= 100 {
            Some(percent)
        } else {
            None
        },
        has_battery,
    }
}

pub fn supply() -> Result<Supply> {
    #[cfg(windows)]
    {
        let mut status = native::SystemPowerStatus::default();
        if unsafe { native::GetSystemPowerStatus(&mut status) } == 0 {
            anyhow::bail!("Não foi possível consultar a alimentação do Windows");
        }
        Ok(decode_supply(status.ac, status.flags, status.percent))
    }
    #[cfg(not(windows))]
    Ok(decode_supply(255, 255, 255))
}

#[cfg(windows)]
mod native {
    pub const CONTINUOUS: u32 = 0x80000000;
    pub const SYSTEM_REQUIRED: u32 = 0x00000001;
    #[repr(C)]
    #[derive(Default)]
    pub struct SystemPowerStatus {
        pub ac: u8,
        pub flags: u8,
        pub percent: u8,
        pub saver: u8,
        pub lifetime: u32,
        pub full_lifetime: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        pub fn SetThreadExecutionState(flags: u32) -> u32;
        pub fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_battery_unknown_and_desktop_without_false_alarms() {
        assert_eq!(decode_supply(0, 1, 42).source, "battery");
        assert_eq!(decode_supply(0, 1, 42).battery_percent, Some(42));
        assert_eq!(decode_supply(1, 8, 100).source, "ac");
        assert_eq!(decode_supply(0, 128, 255).has_battery, Some(false));
        assert_eq!(decode_supply(0, 128, 255).source, "unknown");
        assert_eq!(decode_supply(255, 255, 255).battery_percent, None);
        assert_eq!(decode_supply(0, 255, 42).source, "unknown");
    }

    #[cfg(windows)]
    #[test]
    fn releasing_one_server_keeps_another_thread_protected() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (check_tx, check_rx) = std::sync::mpsc::channel();
        let second = std::thread::spawn(move || {
            let guard = SleepGuard::acquire().unwrap();
            ready_tx.send(()).unwrap();
            check_rx.recv().unwrap();
            let previous = unsafe {
                native::SetThreadExecutionState(native::CONTINUOUS | native::SYSTEM_REQUIRED)
            };
            assert_ne!(previous & native::SYSTEM_REQUIRED, 0);
            drop(guard);
            let previous = unsafe { native::SetThreadExecutionState(native::CONTINUOUS) };
            assert_eq!(previous & native::SYSTEM_REQUIRED, 0);
        });
        ready_rx.recv().unwrap();
        std::thread::spawn(|| {
            drop(SleepGuard::acquire().unwrap());
        })
        .join()
        .unwrap();
        check_tx.send(()).unwrap();
        second.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn native_guard_releases_on_normal_return_and_unwind() {
        // A fresh thread ensures this test cannot clear another operation's request.
        std::thread::spawn(|| {
            for panic in [false, true] {
                let result = std::panic::catch_unwind(|| {
                    let guard = SleepGuard::acquire().unwrap();
                    let previous = unsafe {
                        native::SetThreadExecutionState(
                            native::CONTINUOUS | native::SYSTEM_REQUIRED,
                        )
                    };
                    assert_ne!(previous & native::SYSTEM_REQUIRED, 0);
                    if panic {
                        panic!("test cleanup");
                    }
                    drop(guard);
                });
                assert_eq!(result.is_err(), panic);
                let previous = unsafe { native::SetThreadExecutionState(native::CONTINUOUS) };
                assert_ne!(previous, 0);
                assert_eq!(previous & native::SYSTEM_REQUIRED, 0);
            }
        })
        .join()
        .unwrap();
    }
}
