// @vitest-environment happy-dom
//
// The Android section's updates of the game app: what the row says for what
// the last look found, and the one press it offers.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GameAppStatus } from "../../ipc/bindings";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { useGameAppStatusStore } from "../../shared/gameApp";
import { useGameAppUpdateStore } from "../../shared/gameAppUpdate";
import { AndroidSettingsSection } from "./AndroidSettingsSection";

const { native } = vi.hoisted(() => ({
  native: { gameAppStatus: vi.fn(), gameAppUpdateCheck: vi.fn(), gameAppUpdateInstall: vi.fn() },
}));
vi.mock("../../ipc/client");
vi.mock("../../ipc/native", () => ({ native }));
vi.mock("../../shared/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/platform")>()),
  theGameIsItsOwnApp: () => true,
}));

failOnConsoleError();

function status(patch: { silentUpdate?: boolean; gameRunning?: boolean } = {}): GameAppStatus {
  return {
    installed: true,
    versionName: "0.15.1",
    versionCode: 4459,
    contract: "open",
    gameData: {
      scfaArchives: 31,
      scfaFonts: 4,
      scfaSoundBanks: 76,
      scfaMovies: 22,
      gameVersions: [3841],
      newestKnown: 3841,
      jobStep: null,
      gameRunning: patch.gameRunning ?? false,
      engineCommit: "c4050b171",
    },
    lastGame: null,
    silentUpdate: patch.silentUpdate ?? false,
  };
}

const fill = (text: string, version: string) => text.replace("{version}", version);
const button = (name: string) => screen.getByRole("button", { name });

beforeEach(() => {
  useGameAppStatusStore.setState({ status: null });
  useGameAppUpdateStore.setState({ offer: null, phase: "idle", looked: true, failure: null });
});
afterEach(() => vi.resetAllMocks());

describe("the game app's updates in the Android section", () => {
  it("says Android asks once, and installs on the press", async () => {
    native.gameAppStatus.mockResolvedValue(status());
    native.gameAppUpdateInstall.mockResolvedValue({ kind: "installed" });
    useGameAppUpdateStore.setState({ offer: { version: "0.16.0", metered: false } });
    render(<AndroidSettingsSection />);
    expect(await screen.findByText(fill(en["android.gameApp.update.availableAsks"], "0.16.0"))).toBeTruthy();
    await userEvent.setup().click(button(en["android.gameApp.update.install"]));
    expect(native.gameAppUpdateInstall).toHaveBeenCalledExactlyOnceWith("0.16.0");
    expect(await screen.findByText(en["android.gameApp.update.upToDate"])).toBeTruthy();
  });

  it("holds the update while a game runs", async () => {
    native.gameAppStatus.mockResolvedValue(status({ silentUpdate: true, gameRunning: true }));
    useGameAppUpdateStore.setState({ offer: { version: "0.16.0", metered: false } });
    render(<AndroidSettingsSection />);
    expect(await screen.findByText(fill(en["android.gameApp.update.availableAfterGame"], "0.16.0"))).toBeTruthy();
    expect(button(en["android.gameApp.update.install"]).hasAttribute("disabled")).toBe(true);
  });

  it("looks again on the press when it is up to date", async () => {
    native.gameAppStatus.mockResolvedValue(status());
    native.gameAppUpdateCheck.mockResolvedValue(null);
    render(<AndroidSettingsSection />);
    expect(await screen.findByText(en["android.gameApp.update.upToDate"])).toBeTruthy();
    await userEvent.setup().click(button(en["android.gameApp.update.look"]));
    expect(native.gameAppUpdateCheck).toHaveBeenCalledOnce();
  });
});
