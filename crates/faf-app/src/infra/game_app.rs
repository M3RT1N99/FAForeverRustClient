//! The game app the shell handed over, where the game is an app of its own
//! (see [`crate::ports::game_app`]). Launches look it up when they start, so
//! the order of start-up does not matter.

use std::sync::{Arc, OnceLock};

use crate::ports::game_app::GameAppPort;

static GAME_APP: OnceLock<Arc<dyn GameAppPort>> = OnceLock::new();

/// Start games and replays in `game_app` from now on. Set once at start-up,
/// by the Android shell; a later call is ignored.
pub fn set_game_app(game_app: Arc<dyn GameAppPort>) {
    let _ = GAME_APP.set(game_app);
}

/// The game app, where the game is an app of its own.
pub(crate) fn game_app() -> Option<Arc<dyn GameAppPort>> {
    GAME_APP.get().cloned()
}
