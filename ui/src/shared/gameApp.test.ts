import { beforeEach, expect, it, vi } from "vitest";
import type { GameAppData, GameAppStatus } from "../ipc/bindings";
import { t } from "../i18n";
import {
  GAME_APP_NEED_TEXT,
  GAME_APP_RELEASES,
  gameAppFailureText,
  gameAppNeed,
  gameAppNeedAction,
  playOfflineInGameApp,
  setUpInGameApp,
} from "./gameApp";

const { gameAppStatus, gameAppSetup, gameAppPlay, openHttpsUrl } = vi.hoisted(() => ({
  gameAppStatus: vi.fn(),
  gameAppSetup: vi.fn().mockResolvedValue({ kind: "ended" }),
  gameAppPlay: vi.fn(),
  openHttpsUrl: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../ipc/native", () => ({ native: { gameAppStatus, gameAppSetup, gameAppPlay } }));
vi.mock("./externalLinks", () => ({ openHttpsUrl }));
beforeEach(() => vi.clearAllMocks());

function data(patch: Partial<GameAppData> = {}): GameAppData {
  return {
    scfaArchives: 31,
    scfaFonts: 4,
    scfaSoundBanks: 76,
    scfaMovies: 22,
    gameVersions: [3821],
    newestKnown: 3821,
    jobStep: null,
    gameRunning: false,
    engineCommit: "c4050b171",
    ...patch,
  };
}

function status(patch: Partial<GameAppStatus> = {}): GameAppStatus {
  return {
    installed: true,
    versionName: "0.16.0",
    versionCode: 1600,
    contract: "open",
    gameData: data(),
    lastGame: null,
    ...patch,
  };
}

it.each<[string, GameAppStatus, ReturnType<typeof gameAppNeed>]>([
  ["no game app", status({ installed: false, contract: "none", gameData: null }), "install"],
  ["a game app from before the contract", status({ contract: "none", gameData: null }), "update"],
  ["a game app that refuses this client", status({ contract: "refused", gameData: null }), "allow"],
  ["an open contract without its game data", status({ gameData: null }), "update"],
  ["its setup running", status({ gameData: data({ jobStep: "copying", scfaArchives: 0 }) }), "busy"],
  ["no archives of the player's game", status({ gameData: data({ scfaArchives: 0 }) }), "scfa"],
  ["no fonts of the player's game", status({ gameData: data({ scfaFonts: 0 }) }), "scfa"],
  ["no FAF game files", status({ gameData: data({ gameVersions: [], newestKnown: null }) }), "gameFiles"],
  ["everything there", status(), "ready"],
  ["an older FAF version only", status({ gameData: data({ newestKnown: 3822 }) }), "ready"],
])("needs for %s: %s", (_, given, need) => {
  expect(gameAppNeed(given)).toBe(need);
});

it.each(["install", "update", "allow"] as const)("sends a game app that is %s-ing to its releases", async need => {
  const action = gameAppNeedAction(need);
  expect(action?.label).toBe("android.gameApp.getIt");
  await action?.run();
  expect(openHttpsUrl).toHaveBeenCalledExactlyOnceWith(GAME_APP_RELEASES);
  expect(gameAppSetup).not.toHaveBeenCalled();
});

it.each([
  ["scfa", "scfa"],
  ["gameFiles", "game_files"],
] as const)("opens the game app's own setup for %s", async (need, step) => {
  await gameAppNeedAction(need)?.run();
  expect(gameAppSetup).toHaveBeenCalledExactlyOnceWith(step);
  expect(openHttpsUrl).not.toHaveBeenCalled();
});

it.each(["busy", "ready"] as const)("offers nothing to press when %s", need => {
  expect(gameAppNeedAction(need)).toBeNull();
});

it("says a known failure in the player's words, not the game app's", () => {
  expect(gameAppFailureText("NO_SCFA", "no scfa archives under files/scfa")).toBe(t("android.gameApp.failure.noScfa"));
  expect(gameAppFailureText("RUNNING", "")).toBe(t("android.gameApp.failure.running"));
  expect(gameAppFailureText("NO_MOD", "no file for mod uid")).toBe(t("android.gameApp.failure.noMod"));
});

it("falls back to the game app's own words for a code it does not know", () => {
  expect(gameAppFailureText("NEW_CODE", "something new went wrong")).toBe("something new went wrong");
  expect(gameAppFailureText("NEW_CODE", "")).toBe(t("android.gameApp.failure.unknown", { code: "NEW_CODE" }));
});

it("starts the newest version with nothing set when the game app is ready", async () => {
  gameAppStatus.mockResolvedValueOnce(status());
  gameAppPlay.mockResolvedValueOnce({ kind: "ended" });
  await expect(playOfflineInGameApp()).resolves.toBeUndefined();
  expect(gameAppPlay).toHaveBeenCalledExactlyOnceWith({
    gameVersion: null,
    featuredMod: null,
    arguments: null,
    map: null,
    mods: null,
    modFiles: null,
    rejoinOffer: false,
    rejoinPeers: null,
    rejoinSource: null,
  });
});

it("says what the game app still needs instead of starting it", async () => {
  gameAppStatus.mockResolvedValueOnce(status({ installed: false, contract: "none", gameData: null }));
  await expect(playOfflineInGameApp()).rejects.toThrow(t(GAME_APP_NEED_TEXT.install.title));
  expect(gameAppPlay).not.toHaveBeenCalled();
});

it("rejects with the reason when the game app did not start the game", async () => {
  gameAppStatus.mockResolvedValueOnce(status());
  gameAppPlay.mockResolvedValueOnce({ kind: "failed", failure: "NO_MAP", message: "no map SCMP_009" });
  await expect(playOfflineInGameApp()).rejects.toThrow(t("android.gameApp.failure.noMap"));
});

it("ends quietly when the player cancelled the start with Back", async () => {
  gameAppStatus.mockResolvedValueOnce(status());
  gameAppPlay.mockResolvedValueOnce({ kind: "failed", failure: "CANCELLED", message: "Back during the map's download" });
  await expect(playOfflineInGameApp()).resolves.toBeUndefined();
});

it("settles when the player is back from the setup, and says why it refused a step", async () => {
  await expect(setUpInGameApp("scfa")).resolves.toBeUndefined();
  gameAppSetup.mockResolvedValueOnce({ kind: "failed", failure: "ARGUMENTS", message: "unknown STEP" });
  await expect(setUpInGameApp("game_files")).rejects.toThrow(t("android.gameApp.failure.arguments"));
  expect(gameAppSetup.mock.calls).toEqual([["scfa"], ["game_files"]]);
});

it("passes a bridge failure on for the shared error banner", async () => {
  gameAppStatus.mockRejectedValueOnce(new Error("could not ask the game app: binder died"));
  await expect(playOfflineInGameApp()).rejects.toThrow("could not ask the game app: binder died");
  expect(gameAppPlay).not.toHaveBeenCalled();
});
