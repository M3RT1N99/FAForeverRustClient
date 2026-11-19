// @vitest-environment happy-dom

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { GameAppStatus } from "../ipc/bindings";
import { refreshGameAppStatus, useGameAppStatus, useGameAppStatusStore } from "./gameApp";

const { gameAppStatus, ownApp } = vi.hoisted(() => ({
  gameAppStatus: vi.fn(),
  ownApp: { value: true },
}));
vi.mock("../ipc/native", () => ({ native: { gameAppStatus } }));
vi.mock("./platform", () => ({ theGameIsItsOwnApp: () => ownApp.value }));

beforeEach(() => {
  vi.useFakeTimers();
  ownApp.value = true;
  useGameAppStatusStore.setState({ status: null });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});

function status(jobStep: string | null): GameAppStatus {
  return {
    installed: true,
    versionName: "0.16.0",
    versionCode: 1600,
    contract: "open",
    gameData: {
      scfaArchives: 31,
      scfaFonts: 4,
      scfaSoundBanks: 76,
      scfaMovies: 22,
      gameVersions: [3821],
      newestKnown: 3821,
      jobStep,
      gameRunning: false,
      engineCommit: "c4050b171",
    },
    lastGame: null,
    silentUpdate: false,
  };
}

/** Let the pending status answers land. */
async function settle() {
  await act(async () => {
    await Promise.resolve();
  });
}

function showPage(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
  document.dispatchEvent(new Event("visibilitychange"));
}

it("asks nothing where the game is not its own app", async () => {
  ownApp.value = false;
  const hook = renderHook(() => useGameAppStatus());
  await settle();
  expect(gameAppStatus).not.toHaveBeenCalled();
  expect(hook.result.current.status).toBeNull();
});

it("asks on mount and again when the client comes back to the front", async () => {
  gameAppStatus.mockResolvedValue(status(null));
  const hook = renderHook(() => useGameAppStatus());
  await settle();
  expect(gameAppStatus).toHaveBeenCalledTimes(1);
  expect(hook.result.current.status?.installed).toBe(true);

  showPage("hidden");
  expect(gameAppStatus).toHaveBeenCalledTimes(1);
  showPage("visible");
  await settle();
  expect(gameAppStatus).toHaveBeenCalledTimes(2);

  hook.unmount();
  showPage("visible");
  expect(gameAppStatus).toHaveBeenCalledTimes(2);
});

it("asks every few seconds while the game app's setup runs, and stops after it", async () => {
  gameAppStatus.mockResolvedValueOnce(status("copying")).mockResolvedValue(status(null));
  renderHook(() => useGameAppStatus());
  await settle();
  expect(gameAppStatus).toHaveBeenCalledTimes(1);

  await act(async () => {
    vi.advanceTimersByTime(3000);
    await Promise.resolve();
  });
  expect(gameAppStatus).toHaveBeenCalledTimes(2);

  // The setup has finished: no more asking until something else happens.
  await act(async () => {
    vi.advanceTimersByTime(30000);
    await Promise.resolve();
  });
  expect(gameAppStatus).toHaveBeenCalledTimes(2);
});

it("forgets an answer it can no longer get", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  gameAppStatus.mockResolvedValueOnce(status(null)).mockRejectedValueOnce(new Error("binder died"));
  const hook = renderHook(() => useGameAppStatus());
  await settle();
  expect(hook.result.current.status).not.toBeNull();
  await act(() => hook.result.current.refresh());
  expect(hook.result.current.status).toBeNull();
  expect(warn).toHaveBeenCalledOnce();
  warn.mockRestore();
});

it("gives every view the answer one of them asked for", async () => {
  gameAppStatus.mockResolvedValueOnce(status(null)).mockResolvedValue(status("copying"));
  const banner = renderHook(() => useGameAppStatus());
  const settings = renderHook(() => useGameAppStatus());
  await settle();
  await act(() => settings.result.current.refresh());
  expect(banner.result.current.status?.gameData?.jobStep).toBe("copying");
});

it("asks once more for calls while an answer is on the way, not once for each", async () => {
  let answer: (value: GameAppStatus) => void = () => undefined;
  gameAppStatus
    .mockImplementationOnce(() => new Promise<GameAppStatus>((resolve) => (answer = resolve)))
    .mockResolvedValue(status("copying"));
  const asked = refreshGameAppStatus();
  void refreshGameAppStatus();
  void refreshGameAppStatus();
  answer(status(null));
  await asked;
  expect(gameAppStatus).toHaveBeenCalledTimes(2);
  // The status kept is the later answer, which came after the calls.
  expect(useGameAppStatusStore.getState().status?.gameData?.jobStep).toBe("copying");
});
