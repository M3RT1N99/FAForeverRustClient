// Whether a vault replay can be watched here, and why not.
//
// One rule for every place that offers Watch on one (the cards, the list,
// the detail panel, the recent matchmaker games), so a replay that cannot
// start is not offered anywhere: one the server has not finished processing,
// or one played on a generated map where this device cannot play that yet.

import type { VaultReplay } from "../ipc/bindings";
import { isGeneratedMap } from "./mapPresentation";
import { generatedMapReplaysPlay } from "./platform";

/** Why a replay offers no Watch. */
export type ReplayWatchBlock = "notUploaded" | "generatedMap";

/**
 * Why `replay` offers no Watch, or `null` when it does.
 *
 * The listing names a generated map even when it has no seed for it ("Neroxis
 * Map Generator", the vault's name for a game without a map version), so the
 * row's own map is enough to tell. The backend refuses such a start too
 * (`ports::game_app::map_refusal`), for any way in this misses.
 */
export function replayWatchBlock(replay: Pick<VaultReplay, "replayAvailable" | "map">): ReplayWatchBlock | null {
  if (!replay.replayAvailable) return "notUploaded";
  if (!generatedMapReplaysPlay() && isGeneratedMap(replay.map)) return "generatedMap";
  return null;
}
