//! The page's renderer process ending, as `MainActivity.onRendererGone`
//! reports it: in the client's log, which a phone run sends (logcat does not
//! leave the phone), and with the answer whether the page comes back.
//!
//! Android ends the renderer of a client in the back to free its memory, which
//! a game in front makes likely, and a renderer can crash. Unanswered, the
//! whole client went with it. The page comes back instead: in front at once,
//! by recreating the activity, within the cap the desktop's recovery keeps too
//! ([`RecoveryCap`]), past which the renderer's end is Android's to handle, as
//! it was; in the back once the client is in front again, uncounted
//! ([`RecoveryCap::page_comes_back`] says why), and then in the same activity
//! ([`Java_com_faforever_rustclient_MainActivity_rebuildPage`]).

use std::ffi::c_void;
use std::time::Instant;

use tauri::tao::platform::android::prelude::PACKAGE;
use tauri::wry::prelude::ndk::looper::ThreadLooper;
use tauri::wry::prelude::{JNIEnv, JObject};

use crate::recovery_cap::RecoveryCap;

/// This run's rebuilds, for the cap.
static REBUILDS: RecoveryCap = RecoveryCap::new();

/// `private external fun rendererGone(crashed: Boolean, priorityAtExit: Int, inFront: Boolean): Boolean`
/// of `com.faforever.rustclient.MainActivity`: whether the page comes back,
/// at once in front, once in front again from the back. Only primitives
/// cross, so nothing here touches the Java side.
#[no_mangle]
pub extern "system" fn Java_com_faforever_rustclient_MainActivity_rendererGone(
    _env: *mut c_void,
    _activity: *mut c_void,
    crashed: u8,
    priority_at_exit: i32,
    in_front: u8,
) -> u8 {
    let in_front = in_front != 0;
    let comes_back = REBUILDS.page_comes_back(in_front, Instant::now());
    // Android's `RenderProcessGoneDetail`: a crash, or the system ending it;
    // the priority it had then (0 waived, 1 bound, 2 important).
    tracing::warn!(
        crashed = crashed != 0,
        priority_at_exit,
        in_front,
        comes_back,
        "the page's renderer process is gone"
    );
    u8::from(comes_back)
}

/// `private external fun rebuildPage(): Boolean` of `MainActivity`: build the
/// page again in this activity, whose renderer ended while it was in the
/// back, from `onRestart`. Whether it is under way.
///
/// The page comes from wry's own setup for an activity, run again: tao runs it
/// for every activity it is handed, a recreated one included, and for one it
/// knows, wry builds a webview there anew from the attributes it kept. This
/// one is the same activity, so wry, tao and Tauri keep their window, and
/// Tauri's Android plugins their activity. A recreated one would lose them:
/// Tauri's plugin manager stays with the first activity, whose results
/// launcher and Back handler went with it, and the game app's start and the
/// page's Back would no longer work.
#[no_mangle]
pub extern "system" fn Java_com_faforever_rustclient_MainActivity_rebuildPage(
    env: JNIEnv,
    activity: JObject,
) -> u8 {
    let Some(package) = PACKAGE.get().copied() else {
        tracing::error!("the page cannot be built again: tao does not know the app's package");
        return 0;
    };
    let Some(looper) = ThreadLooper::for_thread() else {
        tracing::error!("the page cannot be built again: not on the activity's thread");
        return 0;
    };
    let activity = match env.new_global_ref(&activity) {
        Ok(activity) => activity,
        Err(error) => {
            tracing::error!(%error, "the page cannot be built again");
            return 0;
        }
    };
    tracing::info!("the page is built again, now the client is in front");
    // SAFETY: called on the activity's own thread, during its `onRestart`.
    // wry's setup makes the web chrome client there, which registers for
    // activity results, and an activity takes that only before it starts.
    unsafe { tauri::wry::android_setup(package, env, &looper, activity) };
    1
}
