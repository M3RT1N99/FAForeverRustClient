// @vitest-environment happy-dom
//
// Where the live view's games come from. The lobby's `game_info` while the
// lobby is connected; while it is closed, as on Android until FAF publishes
// its faf-uid, the games FAF's API lists as running, asked for on opening,
// every minute and on Refresh.

import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Game } from "../../../ipc/bindings";
import { en } from "../../../i18n/catalog/en";
import { failOnConsoleError } from "../../../testing/consoleGuard";
import { clearSentCommands, seedStore, sentCommands } from "../../../testing/mounted";
import { LiveReplayView, RUNNING_GAMES_REFRESH_MS } from "./LiveReplayView";

vi.mock("../../../ipc/client");

failOnConsoleError();

afterEach(() => {
  vi.useRealTimers();
});

function game(id: number, title: string): Game {
  return {
    id,
    title,
    host: "Ada",
    players: 2,
    maxPlayers: 2,
    map: "scmp_009",
    modName: "faf",
    averageRating: 0,
    ratingType: "global",
    passwordProtected: false,
    visibility: "public",
    gameType: "",
    launchedAt: Math.floor(Date.now() / 1000) - 600,
    hostedAt: null,
    ratingMin: null,
    ratingMax: null,
    enforceRatingRange: false,
    teams: { "2": ["Ada"], "3": ["Bert"] },
    simMods: {},
  };
}

/** How many times the view asked for the API's list. */
const runningGameLoads = () =>
  sentCommands().filter((command) => command.kind === "Replays" && command.command.type === "loadRunningGames").length;

describe("the live view's source", () => {
  it("lists the lobby's games while it is connected, and asks the API for nothing", () => {
    seedStore((state) => ({
      ...state,
      lobby: { ...state.lobby, status: "connected", liveGames: [game(1, "From the lobby")] },
      replays: {
        ...state.replays,
        runningGames: [game(2, "From the API")],
        runningGamesStatus: { type: "ready" },
      },
    }));
    render(<LiveReplayView busy={false} />);

    expect(screen.queryAllByText("From the lobby").length).toBeGreaterThan(0);
    expect(screen.queryAllByText("From the API")).toHaveLength(0);
    expect(runningGameLoads()).toBe(0);
    expect(screen.getByText(en["replays.live.updates"])).toBeTruthy();
  });

  it("lists FAF's running games while the lobby is closed, asked on opening, every minute and on Refresh", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    seedStore((state) => ({
      ...state,
      lobby: { ...state.lobby, status: "disconnected", unavailable: "androidNeedsOfficialUid" },
      replays: {
        ...state.replays,
        runningGames: [game(2, "From the API")],
        runningGamesStatus: { type: "ready" },
      },
    }));
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<LiveReplayView busy={false} />);

    expect(screen.queryAllByText("From the API").length).toBeGreaterThan(0);
    expect(runningGameLoads()).toBe(1);

    act(() => {
      vi.advanceTimersByTime(RUNNING_GAMES_REFRESH_MS);
    });
    expect(runningGameLoads()).toBe(2);

    await user.click(screen.getByRole("button", { name: en["replays.live.refresh"] }));
    expect(runningGameLoads()).toBe(3);
  });

  it("says it is waiting for the API's first answer rather than that nothing is being played", () => {
    seedStore((state) => ({
      ...state,
      lobby: { ...state.lobby, status: "disconnected" },
      replays: { ...state.replays, runningGames: [], runningGamesStatus: { type: "loading" } },
    }));
    render(<LiveReplayView busy={false} />);

    expect(screen.getByText(en["replays.live.loadingHint"])).toBeTruthy();
    expect(screen.queryByText(en["replays.live.noneNow"])).toBeNull();
  });

  it("keeps the games last listed under a refresh that failed, and offers another try", async () => {
    seedStore((state) => ({
      ...state,
      lobby: { ...state.lobby, status: "disconnected" },
      replays: {
        ...state.replays,
        runningGames: [game(2, "From the API")],
        runningGamesStatus: { type: "failed", payload: { reason: "not logged in" } },
      },
    }));
    const user = userEvent.setup();
    render(<LiveReplayView busy={false} />);

    expect(screen.getByRole("alert").textContent).toContain(en["replays.live.loadFailed"]);
    expect(screen.queryAllByText("From the API").length).toBeGreaterThan(0);
    clearSentCommands();
    await user.click(screen.getByRole("button", { name: en["common.retry"] }));
    expect(runningGameLoads()).toBe(1);
  });
});
