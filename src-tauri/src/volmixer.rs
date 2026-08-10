// Windows names an app in the Volume Mixer after whichever process owns the audio
// session. Ours is WebView2's renderer, so the mixer reads "Microsoft Edge WebView2" -
// and that exe lives in the shared WebView2 Runtime, so it can't be renamed.
//
// A session does carry its own display name and icon though, and the mixer prefers
// those when they're set. So: find the sessions belonging to our own process tree and
// stamp our name and icon onto them.
//
// Two things create a session we haven't labelled yet - audio starting for the first
// time, and the default output device changing - so we listen for exactly those and do
// nothing in between. No timer.

use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

use windows::core::{implement, Interface, Ref, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, PROPERTYKEY};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionManager2, IAudioSessionNotification, IAudioSessionNotification_Impl,
    IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    DEVICE_STATE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

const DISPLAY_NAME: &str = "YT Music";

// Both callbacks only nudge the worker thread - they never do COM work of their own,
// since blocking inside an audio callback stalls the session it came from. The channel
// is bounded at 1 so a burst of notifications coalesces into a single pass.
#[implement(IAudioSessionNotification)]
struct SessionAdded(SyncSender<()>);

impl IAudioSessionNotification_Impl for SessionAdded_Impl {
    fn OnSessionCreated(&self, _session: Ref<'_, IAudioSessionControl>) -> Result<()> {
        let _ = self.0.try_send(());
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceChanged(SyncSender<()>);

#[allow(non_snake_case)]
impl IMMNotificationClient_Impl for DeviceChanged_Impl {
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> Result<()> {
        if flow == eRender && role == eConsole {
            let _ = self.0.try_send(());
        }
        Ok(())
    }
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

/// pid -> parent pid for every running process.
fn parent_map() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return map;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                map.insert(e.th32ProcessID, e.th32ParentProcessID);
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    map
}

/// Is `pid` us, or anything we spawned (the WebView2 children)?
fn is_ours(pid: u32, us: u32, parents: &HashMap<u32, u32>) -> bool {
    let mut cur = pid;
    // Bounded walk: a corrupt or cyclic snapshot must not hang the thread.
    for _ in 0..32 {
        if cur == us {
            return true;
        }
        match parents.get(&cur) {
            Some(&p) if p != 0 && p != cur => cur = p,
            _ => return false,
        }
    }
    false
}

/// Label every session on this device that belongs to us. Enumerating is also the
/// documented prerequisite for RegisterSessionNotification firing at all, so this
/// always runs before we register.
fn brand(manager: &IAudioSessionManager2, icon: &HSTRING) -> Result<()> {
    unsafe {
        let sessions = manager.GetSessionEnumerator()?;
        let us = std::process::id();
        let parents = parent_map();
        let name = HSTRING::from(DISPLAY_NAME);

        for i in 0..sessions.GetCount()? {
            let Ok(ctl) = sessions.GetSession(i) else { continue };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let Ok(pid) = ctl2.GetProcessId() else { continue };
            if !is_ours(pid, us, &parents) {
                continue;
            }
            let _ = ctl2.SetDisplayName(PCWSTR(name.as_ptr()), std::ptr::null());
            let _ = ctl2.SetIconPath(PCWSTR(icon.as_ptr()), std::ptr::null());
        }
    }
    Ok(())
}

/// What we're currently subscribed to. The registration is per-device, so a default
/// device change means tearing this down and rebuilding it on the new one.
struct Bound {
    id: String,
    manager: IAudioSessionManager2,
    callback: IAudioSessionNotification,
}

fn device_id(device: &windows::Win32::Media::Audio::IMMDevice) -> Result<String> {
    unsafe {
        let raw = device.GetId()?;
        let id = raw.to_string().unwrap_or_default();
        CoTaskMemFree(Some(raw.0 as *const std::ffi::c_void));
        Ok(id)
    }
}

/// Label what's there now, and make sure we're subscribed to the current default
/// device. Errors are non-fatal: we just try again on the next notification.
fn sync(enumerator: &IMMDeviceEnumerator, tx: &SyncSender<()>, icon: &HSTRING, bound: &mut Option<Bound>) {
    unsafe {
        let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
            return;
        };
        let id = device_id(&device).unwrap_or_default();

        if bound.as_ref().is_some_and(|b| b.id == id) {
            if let Some(b) = bound {
                let _ = brand(&b.manager, icon);
            }
            return;
        }

        if let Some(old) = bound.take() {
            let _ = old.manager.UnregisterSessionNotification(&old.callback);
        }
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
            return;
        };
        let _ = brand(&manager, icon);
        let callback: IAudioSessionNotification = SessionAdded(tx.clone()).into();
        if manager.RegisterSessionNotification(&callback).is_ok() {
            *bound = Some(Bound { id, manager, callback });
        }
    }
}

fn watch(icon: HSTRING, tx: SyncSender<()>, rx: Receiver<()>) -> Result<()> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device_cb: IMMNotificationClient = DeviceChanged(tx.clone()).into();
        enumerator.RegisterEndpointNotificationCallback(&device_cb)?;

        let mut bound: Option<Bound> = None;
        loop {
            sync(&enumerator, &tx, &icon, &mut bound);
            // Blocks until a session or the default device actually changes.
            if rx.recv().is_err() {
                return Ok(());
            }
        }
    }
}

/// Start labelling our audio sessions. Runs for the life of the process.
pub fn spawn() {
    std::thread::spawn(|| {
        let icon = HSTRING::from(
            std::env::current_exe()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
        );
        unsafe {
            // MTA: the callbacks arrive on pool threads, so there's no message loop here.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let (tx, rx) = sync_channel::<()>(1);
        let _ = watch(icon, tx, rx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_process_tree() {
        // 100 -> 50 -> 10(us): a webview grandchild counts as ours.
        let parents = HashMap::from([(100, 50), (50, 10), (10, 4), (7, 4)]);
        assert!(is_ours(100, 10, &parents));
        assert!(is_ours(10, 10, &parents));
        // A sibling process under the same root is not ours.
        assert!(!is_ours(7, 10, &parents));
        // Unknown pid, and a cycle, both terminate instead of hanging.
        assert!(!is_ours(999, 10, &parents));
        assert!(!is_ours(1, 10, &HashMap::from([(1, 2), (2, 1)])));
    }

    #[test]
    fn notification_signal_coalesces() {
        // The bounded channel is what keeps a burst of OnSessionCreated calls from
        // queueing up passes, and try_send is what keeps a callback from ever blocking.
        let (tx, rx) = sync_channel::<()>(1);
        let cb = SessionAdded(tx);
        for _ in 0..100 {
            let _ = cb.0.try_send(());
        }
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err(), "should coalesce to a single pass");
    }
}
