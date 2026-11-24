// Whether this build offers play, asked of the shell once at startup.
//
// The answer lives in `platform.ts` beside the other facts about the build,
// and is read there (`playIsOffered`). The asking is here because it goes
// through `ipc/native`, which itself reads `platform.ts`: asking from there
// would make the two modules import each other.

import { native } from "../ipc/native";
import { setPlayOffered } from "./platform";
import { isDesktopShell } from "./webviewEngine";

/**
 * Ask the shell before the first render, so that no way into play is drawn
 * and then taken away. Outside the shell, or from one without the command,
 * the platform's own answer stands.
 */
export async function loadPlayOffer(): Promise<void> {
  if (!isDesktopShell()) return;
  try {
    setPlayOffered(await native.playOffered());
  } catch {
    setPlayOffered(null);
  }
}
