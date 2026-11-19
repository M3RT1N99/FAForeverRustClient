// The FAF game app: the game this client starts on Android.
//
// On Android the game is an app of its own (io.github.m3rt1n99.fafre), with
// its own files. The client asks it what it has (its status provider), sends
// the player to its screen that sets the game data up, and starts games in it
// (`native.gameApp*`). This is what those answers mean for the page.

import { useEffect } from "react";
import { create } from "zustand";
import type { MessageKey } from "../i18n";
import { t } from "../i18n";
import { native, type GameAppStatus } from "../ipc/native";
import { openHttpsUrl } from "./externalLinks";
import { theGameIsItsOwnApp } from "./platform";

/** Where the game app is published. */
export const GAME_APP_RELEASES = "https://github.com/M3RT1N99/faf-re/releases";

/**
 * What the game app needs before a game can start, the first that applies:
 * the app itself, a version with the launcher contract, to let this client
 * in, the player's Supreme Commander, FAF's game files. `busy` while its own
 * setup runs, `ready` when nothing is missing.
 */
export type GameAppNeed = "install" | "update" | "allow" | "busy" | "scfa" | "gameFiles" | "ready";

export function gameAppNeed(status: GameAppStatus): GameAppNeed {
  if (!status.installed) return "install";
  if (status.contract === "none") return "update";
  if (status.contract === "refused") return "allow";
  const data = status.gameData;
  if (!data) return "update";
  if (data.jobStep) return "busy";
  if (data.scfaArchives === 0 || data.scfaFonts === 0) return "scfa";
  if (data.gameVersions.length === 0) return "gameFiles";
  return "ready";
}

/** What a need says: a title, and a line of what to do about it. */
export const GAME_APP_NEED_TEXT: Record<Exclude<GameAppNeed, "ready">, { title: MessageKey; hint?: MessageKey }> = {
  install: { title: "android.gameApp.need.install", hint: "android.gameApp.need.installHint" },
  update: { title: "android.gameApp.need.update", hint: "android.gameApp.need.updateHint" },
  allow: { title: "android.gameApp.need.allow", hint: "android.gameApp.need.allowHint" },
  busy: { title: "android.gameApp.need.busy" },
  scfa: { title: "android.gameApp.need.scfa", hint: "android.gameApp.need.scfaHint" },
  gameFiles: { title: "android.gameApp.need.gameFiles", hint: "android.gameApp.need.gameFilesHint" },
};

/**
 * The one press that answers a need, if there is one: the game app's
 * releases for the app itself, its own setup for its game data.
 */
export function gameAppNeedAction(need: GameAppNeed): { label: MessageKey; run: () => Promise<void> } | null {
  switch (need) {
    case "install":
    case "update":
    case "allow":
      return { label: "android.gameApp.getIt", run: () => openHttpsUrl(GAME_APP_RELEASES) };
    case "scfa":
      return { label: "android.gameApp.scfaSetUp", run: () => setUpInGameApp("scfa") };
    case "gameFiles":
      return { label: "android.gameApp.gameFilesGet", run: () => setUpInGameApp("game_files") };
    case "busy":
    case "ready":
      return null;
  }
}

/** The game app's failure codes, as the player reads them. */
const FAILURES: Record<string, MessageKey> = {
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
};

/**
 * Why the game app did not start a game, in the player's language. A code
 * this client does not know yet falls back to the game app's own words.
 */
export function gameAppFailureText(failure: string, message: string): string {
  const key = FAILURES[failure];
  if (key) return t(key);
  return message || t("android.gameApp.failure.unknown", { code: failure });
}

/**
 * Back during the game app's downloads before a start: the player's own
 * choice, not a failure to report.
 */
const CANCELLED = "CANCELLED";

/**
 * Open the game app's own screen for a step of its game data. Settles when
 * the player is back; a job begun there goes on in the game app, which its
 * status shows.
 */
export async function setUpInGameApp(step: "scfa" | "game_files"): Promise<void> {
  const end = await native.gameAppSetup(step);
  if (end.kind === "failed") throw new Error(gameAppFailureText(end.failure, end.message));
}

/**
 * Start an offline game in the game app, the newest version. Settles when it
 * has ended; rejects with the reason in the player's language when the game
 * app did not start it.
 *
 * Its status is asked again first rather than trusted from the page: the game
 * app may have gone, or finished its setup, since the page last looked, and a
 * game app without the contract has no start to answer with a reason.
 */
export async function playOfflineInGameApp(): Promise<void> {
  const need = gameAppNeed(await native.gameAppStatus());
  if (need !== "ready") throw new Error(t(GAME_APP_NEED_TEXT[need].title));
  const end = await native.gameAppPlay({
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
  if (end.kind === "failed" && end.failure !== CANCELLED) {
    throw new Error(gameAppFailureText(end.failure, end.message));
  }
}

/** How often the status is asked again while the game app's setup runs. */
const BUSY_POLL_MS = 3000;

/**
 * The game app's status as last asked, one for every view that shows it, so
 * that an answer one of them asked for (after the setup screen, say) reaches
 * the others too. Views read it through `useGameAppStatus`.
 */
export const useGameAppStatusStore = create<{ status: GameAppStatus | null }>(() => ({ status: null }));

let asking: Promise<void> | null = null;
let askAgain = false;

/**
 * Ask the game app for its status, and give every view the answer: `null`
 * when it could not be asked. A call while an answer is on the way asks once
 * more after it, so the status kept is never older than the call.
 */
export function refreshGameAppStatus(): Promise<void> {
  if (!theGameIsItsOwnApp()) return Promise.resolve();
  if (asking) {
    askAgain = true;
    return asking;
  }
  asking = (async () => {
    do {
      askAgain = false;
      const status = await native.gameAppStatus().catch((error: unknown) => {
        console.warn("could not ask the game app for its status", error);
        return null;
      });
      useGameAppStatusStore.setState({ status });
    } while (askAgain);
  })().finally(() => {
    asking = null;
  });
  return asking;
}

/**
 * The game app's status, on Android; `null` elsewhere, and until the first
 * answer. Asked again when a view showing it mounts, when the client comes
 * back to the front (from the game app's setup or a game), and every few
 * seconds while its setup runs.
 */
export function useGameAppStatus(): { status: GameAppStatus | null; refresh: () => Promise<void> } {
  const status = useGameAppStatusStore((state) => state.status);

  useEffect(() => {
    void refreshGameAppStatus();
    const onVisible = () => {
      if (document.visibilityState === "visible") void refreshGameAppStatus();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, []);

  const busy = status ? gameAppNeed(status) === "busy" : false;
  useEffect(() => {
    if (!busy) return undefined;
    const timer = window.setInterval(() => void refreshGameAppStatus(), BUSY_POLL_MS);
    return () => window.clearInterval(timer);
  }, [busy]);

  return { status, refresh: refreshGameAppStatus };
}
