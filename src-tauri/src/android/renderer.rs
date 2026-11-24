//! The page's renderer process ending, as `MainActivity.onRendererGone`
//! reports it: in the client's log, which a phone run sends (logcat does not
//! leave the phone), and with the answer whether the activity is built again.
//!
//! Android ends the renderer of a client in the back to free its memory, which
//! a game in front makes likely, and a renderer can crash. Unanswered, the
//! whole client went with it. The activity rebuilds the page instead, within
//! the cap the desktop's recovery keeps too ([`RecoveryCap`]); past it, the
//! renderer's end is Android's to handle, as it was.

use std::ffi::c_void;
use std::time::Instant;

use crate::recovery_cap::RecoveryCap;

/// This run's rebuilds, for the cap.
static REBUILDS: RecoveryCap = RecoveryCap::new();

/// `private external fun rendererGone(crashed: Boolean, priorityAtExit: Int): Boolean`
/// of `com.faforever.rustclient.MainActivity`: whether to build the activity
/// again. Only primitives cross, so nothing here touches the Java side.
#[no_mangle]
pub extern "system" fn Java_com_faforever_rustclient_MainActivity_rendererGone(
    _env: *mut c_void,
    _activity: *mut c_void,
    crashed: u8,
    priority_at_exit: i32,
) -> u8 {
    let rebuild = REBUILDS.allow(Instant::now());
    // Android's `RenderProcessGoneDetail`: a crash, or the system ending it;
    // the priority it had then (0 waived, 1 bound, 2 important).
    tracing::warn!(
        crashed = crashed != 0,
        priority_at_exit,
        rebuild,
        "the page's renderer process is gone"
    );
    u8::from(rebuild)
}
