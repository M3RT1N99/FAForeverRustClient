// What the client can tell about the machine it is running on.
//
// The backend knows this properly and could send it, but nothing in the state
// needs it: the only question the interface asks is whether Forged Alliance
// has to be started through Wine, and that is decided by the user agent of the
// webview the client is drawn in, which is the operating system's own.

/**
 * Whether the game on this machine is a Windows executable that needs a
 * wrapper to run.
 *
 * True everywhere except Windows and Android. FAF ships no native build of the
 * engine for the desktop, so on Linux (and on macOS, where the same is true) a
 * launch has to go through Wine or Proton, and the settings that describe one
 * are worth showing. On Windows the game runs directly unless the player names
 * a program of their own to run it through. On Android the game is its own app
 * (see [`theGameIsItsOwnApp`]), whose user agent says Linux too.
 *
 * Deliberately a negative test: an unfamiliar user agent shows the settings
 * rather than hiding them, and a setting nobody needs is a smaller failure
 * than a setting somebody needs and cannot reach.
 */
export function gameNeedsALaunchWrapper(): boolean {
  return !runsOnWindows() && !theGameIsItsOwnApp();
}

/**
 * Whether Forged Alliance is an app of its own on this device: FAF's game app
 * on Android, which keeps its own files. The client cannot be pointed at its
 * folders, so it offers no folder or executable to choose for it.
 */
export function theGameIsItsOwnApp(): boolean {
  return runsOnAndroid();
}

/**
 * Whether the system's dialog can pick a folder. Android's cannot, and the
 * shell refuses the request (`pick_path`), so nothing there offers one.
 */
export function foldersCanBePicked(): boolean {
  return !runsOnAndroid();
}

/**
 * Whether the client starts programs of its own beside the game: the ICE
 * adapter (Java's faf-ice-adapter or Go's faf-pioneer) and the player's
 * companion programs.
 *
 * Not on Android. Neither adapter is in the app, the game there connects to the
 * other players by itself (from a later version on), and an app cannot start
 * a program it did not ship.
 */
export function startsProgramsBesideTheGame(): boolean {
  return !runsOnAndroid();
}

/**
 * Whether the client is running on Windows.
 */
export function runsOnWindows(): boolean {
  const agent = typeof navigator === "undefined" ? "" : navigator.userAgent;
  return /Windows/i.test(agent);
}

/**
 * Whether the Steam status can work here: Windows and Linux, the two systems
 * the client is released for and ships Valve's library on.
 *
 * The opposite default from [`gameNeedsALaunchWrapper`]: an unfamiliar user
 * agent hides the setting, because on any other system switching it on would
 * do nothing at all. Android's user agent says Linux too, and is not one.
 */
export function steamStatusIsSupported(): boolean {
  return runsOnWindows() || (/Linux/i.test(userAgent()) && !runsOnAndroid());
}

/**
 * Whether the client is the Android app: Tauri's mobile build in the system
 * webview, whose user agent names Android.
 */
export function runsOnAndroid(): boolean {
  return /Android/i.test(userAgent());
}

function userAgent(): string {
  return typeof navigator === "undefined" ? "" : navigator.userAgent;
}
