// The FAF game app's failure codes, its launcher contract's `failure`, as the
// player reads them. Shared by the starts the page asks for itself
// (`shared/gameApp`) and those the backend reports as a reason (`plainError`).

import type { MessageKey } from "../i18n";

export const GAME_APP_FAILURES: Readonly<Record<string, MessageKey>> = {
  NO_SCFA: "android.gameApp.failure.noScfa",
  NO_FONTS: "android.gameApp.failure.noScfa",
  NO_GAME_FILES: "android.gameApp.failure.noGameFiles",
  INIT_FILE: "android.gameApp.failure.broken",
  ENVIRONMENT: "android.gameApp.failure.broken",
  STORAGE: "android.gameApp.failure.storage",
  ARGUMENTS: "android.gameApp.failure.arguments",
  RUNNING: "android.gameApp.failure.running",
  PROCESS_ENDED: "android.gameApp.failure.processEnded",
  CALLER: "android.gameApp.failure.caller",
  NO_MAP: "android.gameApp.failure.noMap",
  NO_MOD: "android.gameApp.failure.noMod",
  DOWNLOAD: "android.gameApp.failure.download",
  REPLAY: "android.gameApp.failure.replay",
  CANCELLED: "android.gameApp.failure.cancelled",
};

/**
 * A launch's failure as the Android shell words a refusal of the game app's
 * (`failure_reason` in `src-tauri/src/game_app.rs`): the game app's own
 * words, and its code in brackets at the end. An online game's failure has
 * the launcher's words before them (`game launch: ...`).
 */
const SHELL_REASON = /\bFAF's game app did not start it\b[\s\S]*\(([A-Z_]+)\)$/;

/** What a backend reason that is a refusal of the game app's says, if one. */
export function gameAppFailureIn(reason: string): MessageKey | undefined {
  const code = SHELL_REASON.exec(reason)?.[1];
  return code === undefined ? undefined : GAME_APP_FAILURES[code];
}
