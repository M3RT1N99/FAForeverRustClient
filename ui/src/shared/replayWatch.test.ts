// Which vault replays offer Watch: one the server has finished processing,
// and on Android not one played on a generated map, which the game app there
// cannot have (it refused such a start at once, NO_MAP).

import { afterEach, describe, expect, it, vi } from "vitest";
import { replayWatchBlock } from "./replayWatch";

const ANDROID =
  "Mozilla/5.0 (Linux; Android 16; SM-S908B Build/BP2A.250605.031; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/142.0.0.0 Mobile Safari/537.36";
const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";

const SEEDED = "neroxis_map_generator_1.12.1_aaaaaaaaaaaaa_aiea";
/** The vault's name for a game without a map version: a generated map without its seed. */
const SEEDLESS = "Neroxis Map Generator";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("replayWatchBlock", () => {
  it("offers no Watch on Android for a replay on a generated map, seeded or not", () => {
    vi.stubGlobal("navigator", { userAgent: ANDROID });
    expect(replayWatchBlock({ replayAvailable: true, map: SEEDED })).toBe("generatedMap");
    expect(replayWatchBlock({ replayAvailable: true, map: SEEDLESS })).toBe("generatedMap");
    expect(replayWatchBlock({ replayAvailable: true, map: "setons_clutch.v0002" })).toBeNull();
  });

  it("offers Watch for a generated map where the map is generated before the start", () => {
    vi.stubGlobal("navigator", { userAgent: LINUX });
    expect(replayWatchBlock({ replayAvailable: true, map: SEEDED })).toBeNull();
    expect(replayWatchBlock({ replayAvailable: true, map: SEEDLESS })).toBeNull();
  });

  it("says first that a replay is not uploaded yet, everywhere", () => {
    for (const userAgent of [ANDROID, LINUX]) {
      vi.stubGlobal("navigator", { userAgent });
      expect(replayWatchBlock({ replayAvailable: false, map: SEEDED })).toBe("notUploaded");
      expect(replayWatchBlock({ replayAvailable: false, map: "setons_clutch.v0002" })).toBe("notUploaded");
    }
  });
});
