// Keeping FAF's game app up to date on Android.
//
// The client looks for a newer release of the game app when it starts and
// every few hours after (`useGameAppUpdates`, mounted once at the root), and
// has Android install it (`native.gameAppUpdate*`). By itself where Android
// updates without asking, which it does once this client installed the game
// app last, and the network is not metered; on the player's press otherwise,
// and then Android asks its own question first. Never while a game runs: the
// update ends the game app's process.

import { useEffect } from "react";
import { create } from "zustand";
import type { MessageKey } from "../i18n";
import { t } from "../i18n";
import { native, type GameAppStatus, type GameAppUpdate } from "../ipc/native";
import { refreshGameAppStatus, useGameAppStatusStore } from "./gameApp";
import { plainError } from "./plainError";
import { theGameIsItsOwnApp } from "./platform";

/** How often the client looks again while it runs. */
export const LOOK_EVERY_MS = 4 * 60 * 60 * 1000;

export type GameAppUpdatePhase = "idle" | "looking" | "installing";

interface GameAppUpdateState {
  /** The newer release the last look found, until it is installed. */
  offer: GameAppUpdate | null;
  phase: GameAppUpdatePhase;
  /** Whether a look has answered since the client started. */
  looked: boolean;
  /** Why the last look or install did not work, in the player's words. */
  failure: string | null;
}

export const useGameAppUpdateStore = create<GameAppUpdateState>(() => ({
  offer: null,
  phase: "idle",
  looked: false,
  failure: null,
}));

function reasonOf(error: unknown): string {
  return plainError(error instanceof Error ? error.message : String(error));
}

/** Why an update was not installed, by the client's codes, as the player reads it. */
const INSTALL_FAILURES: Record<string, MessageKey> = {
  RUNNING: "android.gameApp.update.failure.running",
  PACKAGE: "android.gameApp.update.failure.package",
  SIGNER: "android.gameApp.update.failure.signer",
  NOT_NEWER: "android.gameApp.update.failure.notNewer",
  CONFLICT: "android.gameApp.update.failure.conflict",
  STORAGE: "android.gameApp.update.failure.storage",
  INCOMPATIBLE: "android.gameApp.update.failure.incompatible",
};

export function installFailureText(failure: string, message: string): string {
  const key = INSTALL_FAILURES[failure];
  if (key) return t(key);
  return t("android.gameApp.update.failure.install", { reason: message || failure });
}

/** Look for a newer release now. */
export async function lookForGameAppUpdate(): Promise<GameAppUpdate | null> {
  const store = useGameAppUpdateStore;
  if (store.getState().phase !== "idle") return store.getState().offer;
  store.setState({ phase: "looking", failure: null });
  try {
    const offer = await native.gameAppUpdateCheck();
    store.setState({ offer, phase: "idle", looked: true });
    return offer;
  } catch (error) {
    store.setState({ phase: "idle", failure: reasonOf(error) });
    return null;
  }
}

/**
 * Download and install the release the last look found. The player saying
 * no to Android's question is no failure; the offer stays for another press.
 */
export async function installGameAppUpdate(): Promise<void> {
  const store = useGameAppUpdateStore;
  const { offer, phase } = store.getState();
  if (!offer || phase !== "idle") return;
  store.setState({ phase: "installing", failure: null });
  try {
    const end = await native.gameAppUpdateInstall(offer.version);
    if (end.kind === "installed") store.setState({ offer: null });
    else if (end.failure !== "ABORTED") store.setState({ failure: installFailureText(end.failure, end.message) });
  } catch (error) {
    store.setState({ failure: reasonOf(error) });
  } finally {
    store.setState({ phase: "idle" });
    await refreshGameAppStatus();
  }
}

/**
 * Whether an update goes in without the player: Android updates the game app
 * without asking, the network is not metered, no game runs (which only a game
 * app with the contract can say), and the client is in front, where Android
 * would show a question after all.
 */
export function installsByItself(offer: GameAppUpdate, status: GameAppStatus | null): boolean {
  return (
    !!status?.silentUpdate &&
    !offer.metered &&
    status.gameData !== null &&
    !status.gameData.gameRunning &&
    document.visibilityState === "visible"
  );
}

async function lookAndMaybeInstall(): Promise<void> {
  // Fresh: whether a game runs, and whether Android would ask.
  await refreshGameAppStatus();
  const offer = await lookForGameAppUpdate();
  if (offer && installsByItself(offer, useGameAppStatusStore.getState().status)) {
    await installGameAppUpdate();
  }
}

/**
 * Looks for a newer game app at the client's start and every `LOOK_EVERY_MS`
 * after, and installs it where it goes in by itself. Mounted once, at the
 * root. A start is one request to GitHub, which allows sixty an hour.
 */
export function useGameAppUpdates(): void {
  useEffect(() => {
    if (!theGameIsItsOwnApp()) return undefined;
    void lookAndMaybeInstall();
    const timer = window.setInterval(() => void lookAndMaybeInstall(), LOOK_EVERY_MS);
    return () => window.clearInterval(timer);
  }, []);
}
