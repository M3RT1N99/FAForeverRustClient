// @vitest-environment happy-dom

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GameAppStatus, GameAppUpdate } from "../ipc/bindings";
import { t } from "../i18n";
import { useGameAppStatusStore } from "./gameApp";
import {
  LOOK_EVERY_MS,
  installFailureText,
  installGameAppUpdate,
  installsByItself,
  lookForGameAppUpdate,
  useGameAppUpdateStore,
  useGameAppUpdates,
} from "./gameAppUpdate";

const { native, ownApp } = vi.hoisted(() => ({
  native: { gameAppStatus: vi.fn(), gameAppUpdateCheck: vi.fn(), gameAppUpdateInstall: vi.fn() },
  ownApp: { value: true },
}));
vi.mock("../ipc/native", () => ({ native }));
vi.mock("./platform", () => ({ theGameIsItsOwnApp: () => ownApp.value }));

function status(patch: Partial<GameAppStatus> = {}, running = false): GameAppStatus {
  return {
    installed: true,
    versionName: "0.15.1",
    versionCode: 5590,
    contract: "open",
    gameData: {
      scfaArchives: 31,
      scfaFonts: 4,
      scfaSoundBanks: 76,
      scfaMovies: 22,
      gameVersions: [3841],
      newestKnown: 3841,
      jobStep: null,
      gameRunning: running,
      engineCommit: "c4050b171",
    },
    lastGame: null,
    silentUpdate: true,
    ...patch,
  };
}

const OFFER: GameAppUpdate = { version: "0.16.0", metered: false };

function showPage(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
}

beforeEach(() => {
  ownApp.value = true;
  showPage("visible");
  useGameAppStatusStore.setState({ status: null });
  useGameAppUpdateStore.setState({ offer: null, phase: "idle", looked: false, failure: null });
  native.gameAppStatus.mockResolvedValue(status());
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.resetAllMocks();
});

describe("a look for a newer game app", () => {
  it("keeps what it found", async () => {
    native.gameAppUpdateCheck.mockResolvedValueOnce(OFFER);
    await expect(lookForGameAppUpdate()).resolves.toEqual(OFFER);
    expect(useGameAppUpdateStore.getState()).toMatchObject({ offer: OFFER, phase: "idle", looked: true });
    native.gameAppUpdateCheck.mockResolvedValueOnce(null);
    await lookForGameAppUpdate();
    expect(useGameAppUpdateStore.getState()).toMatchObject({ offer: null, looked: true });
  });

  it("says plainly why it could not look", async () => {
    native.gameAppUpdateCheck.mockRejectedValueOnce(
      new Error("could not reach the game app's releases: error sending request for url"),
    );
    await expect(lookForGameAppUpdate()).resolves.toBeNull();
    expect(useGameAppUpdateStore.getState().failure).toBe(t("errors.cause.offline"));
  });

  it("does not ask twice at once", async () => {
    let answer: (offer: GameAppUpdate | null) => void = () => undefined;
    native.gameAppUpdateCheck.mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
    const first = lookForGameAppUpdate();
    await lookForGameAppUpdate();
    answer(OFFER);
    await first;
    expect(native.gameAppUpdateCheck).toHaveBeenCalledOnce();
  });
});

describe("an update of the game app", () => {
  beforeEach(() => useGameAppUpdateStore.setState({ offer: OFFER, looked: true }));

  it("drops the offer once installed, and asks the game app's status again", async () => {
    native.gameAppUpdateInstall.mockResolvedValueOnce({ kind: "installed" });
    await installGameAppUpdate();
    expect(native.gameAppUpdateInstall).toHaveBeenCalledExactlyOnceWith("0.16.0");
    expect(useGameAppUpdateStore.getState()).toMatchObject({ offer: null, phase: "idle", failure: null });
    expect(native.gameAppStatus).toHaveBeenCalled();
  });

  it("says why it was not installed, and keeps the offer", async () => {
    native.gameAppUpdateInstall.mockResolvedValueOnce({ kind: "failed", failure: "SIGNER", message: "d00d" });
    await installGameAppUpdate();
    expect(useGameAppUpdateStore.getState()).toMatchObject({
      offer: OFFER,
      failure: t("android.gameApp.update.failure.signer"),
    });
  });

  it("takes the player's no to Android's question as no failure", async () => {
    native.gameAppUpdateInstall.mockResolvedValueOnce({ kind: "failed", failure: "ABORTED", message: "" });
    await installGameAppUpdate();
    expect(useGameAppUpdateStore.getState()).toMatchObject({ offer: OFFER, failure: null });
  });

  it("does nothing without an offer", async () => {
    useGameAppUpdateStore.setState({ offer: null });
    await installGameAppUpdate();
    expect(native.gameAppUpdateInstall).not.toHaveBeenCalled();
  });

  it("names Android's own reason where the client has no words of its own", () => {
    expect(installFailureText("NOT_NEWER", "5590 after 5600")).toBe(t("android.gameApp.update.failure.notNewer"));
    expect(installFailureText("INSTALL", "INSTALL_FAILED_VERIFICATION_FAILURE")).toBe(
      t("android.gameApp.update.failure.install", { reason: "INSTALL_FAILED_VERIFICATION_FAILURE" }),
    );
  });
});

describe("an update without the player", () => {
  it.each<[string, GameAppUpdate, GameAppStatus | null, boolean]>([
    ["on Wi-Fi, where Android does not ask", OFFER, status(), true],
    ["where Android would ask first", OFFER, status({ silentUpdate: false }), false],
    ["on a metered network", { version: "0.16.0", metered: true }, status(), false],
    ["during a game", OFFER, status({}, true), false],
    ["for a game app that cannot say whether a game runs", OFFER, status({ contract: "none", gameData: null }), false],
    ["before the status is known", OFFER, null, false],
  ])("%s: %s", (_, offer, given, expected) => {
    expect(installsByItself(offer, given)).toBe(expected);
  });

  it("waits while the client is not in front", () => {
    showPage("hidden");
    expect(installsByItself(OFFER, status())).toBe(false);
  });
});

describe("the looks at the start and every few hours", () => {
  it("looks at the start and installs what goes in by itself", async () => {
    native.gameAppUpdateCheck.mockResolvedValue(OFFER);
    native.gameAppUpdateInstall.mockResolvedValue({ kind: "installed" });
    renderHook(() => useGameAppUpdates());
    await vi.waitFor(() => expect(native.gameAppUpdateInstall).toHaveBeenCalledOnce());
  });

  it("leaves a metered download to the player", async () => {
    native.gameAppUpdateCheck.mockResolvedValue({ version: "0.16.0", metered: true });
    renderHook(() => useGameAppUpdates());
    await vi.waitFor(() => expect(useGameAppUpdateStore.getState().offer).not.toBeNull());
    expect(native.gameAppUpdateInstall).not.toHaveBeenCalled();
  });

  it("looks again only after hours", async () => {
    vi.useFakeTimers();
    native.gameAppUpdateCheck.mockResolvedValue(null);
    renderHook(() => useGameAppUpdates());
    await act(async () => {
      await vi.advanceTimersByTimeAsync(LOOK_EVERY_MS - 1);
    });
    expect(native.gameAppUpdateCheck).toHaveBeenCalledOnce();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(native.gameAppUpdateCheck).toHaveBeenCalledTimes(2);
  });

  it("looks nowhere the game is not its own app", async () => {
    ownApp.value = false;
    renderHook(() => useGameAppUpdates());
    await act(async () => {
      await Promise.resolve();
    });
    expect(native.gameAppUpdateCheck).not.toHaveBeenCalled();
  });
});
