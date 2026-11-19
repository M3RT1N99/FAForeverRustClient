// @vitest-environment happy-dom
//
// Play offline on Android, where the game is FAF's game app: the login screen
// says what that app still needs and starts games in it, never through the
// desktop's offline launch.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GameAppStatus } from "../../ipc/bindings";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { clearSentCommands, sentCommands } from "../../testing/mounted";
import { LoginView } from "./LoginView";

const { gameAppStatus, gameAppPlay } = vi.hoisted(() => ({ gameAppStatus: vi.fn(), gameAppPlay: vi.fn() }));
vi.mock("../../ipc/client");
vi.mock("../../ipc/native", () => ({ native: { gameAppStatus, gameAppPlay } }));
vi.mock("../../shared/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/platform")>()),
  theGameIsItsOwnApp: () => true,
}));

failOnConsoleError();

beforeEach(() => {
  vi.clearAllMocks();
  clearSentCommands();
});

const READY: GameAppStatus = {
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
    jobStep: null,
    gameRunning: false,
    engineCommit: "c4050b171",
  },
  lastGame: null,
};

const MISSING: GameAppStatus = {
  installed: false,
  versionName: "",
  versionCode: 0,
  contract: "none",
  gameData: null,
  lastGame: null,
};

const playButton = () => screen.getByRole("button", { name: en["auth.playOffline"] });

describe("Play offline where the game is its own app", () => {
  it("says the game app is missing and waits for it", async () => {
    gameAppStatus.mockResolvedValue(MISSING);
    render(<LoginView />);
    expect(await screen.findByText(en["android.gameApp.need.install"])).toBeTruthy();
    expect(playButton().hasAttribute("disabled")).toBe(true);
    expect(screen.queryByText(en["auth.offlineSetupMissing"])).toBeNull();
  });

  it("starts the game in the game app, not through the desktop launch", async () => {
    gameAppStatus.mockResolvedValue(READY);
    gameAppPlay.mockResolvedValue({ kind: "ended" });
    const user = userEvent.setup();
    render(<LoginView />);
    await user.click(playButton());
    expect(gameAppPlay).toHaveBeenCalledOnce();
    expect(sentCommands().filter((command) => command.kind === "Auth")).toEqual([]);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says why the game app did not start the game, in the player's words", async () => {
    gameAppStatus.mockResolvedValue(READY);
    gameAppPlay.mockResolvedValue({ kind: "failed", failure: "NO_SCFA", message: "no scfa archives" });
    const user = userEvent.setup();
    render(<LoginView />);
    await user.click(playButton());
    expect(await screen.findByText(en["android.gameApp.failure.noScfa"])).toBeTruthy();
    expect(screen.queryByText("no scfa archives")).toBeNull();
  });
});
