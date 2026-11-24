// @vitest-environment happy-dom
//
// What an Android build without the play flag shows: replays and everything
// else, and no way into hosting, joining or the matchmaker. Play is hidden,
// not restyled: what stays looks as it did.

import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Game } from "../../ipc/bindings";
import { en } from "../../i18n/catalog/en";
import { renderBody } from "../chat/messages/chatFormat";
import { openTabForMode, TAB_ORDER, tabsForMode } from "../nav/tabs";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { seedStore } from "../../testing/mounted";
import { usePlayerMenu } from "../../shared/hooks/usePlayerMenu";
import { playIsOffered, setPlayOffered } from "../../shared/platform";

vi.mock("../../ipc/client");

failOnConsoleError();

beforeEach(() => {
  // What the shell of an Android release answers (`play_offered`).
  setPlayOffered(false);
});

afterEach(() => {
  setPlayOffered(null);
});

function game(id: number, host: string, players: string[]): Game {
  return {
    id,
    title: `${host}'s game`,
    host,
    players: players.length,
    maxPlayers: 8,
    map: "scmp_009",
    modName: "faf",
    averageRating: 1_200,
    ratingType: "global",
    passwordProtected: false,
    visibility: "public",
    gameType: "custom",
    launchedAt: null,
    hostedAt: null,
    ratingMin: null,
    ratingMax: null,
    enforceRatingRange: false,
    teams: { "1": players },
    simMods: {},
  };
}

const player = (id: number, login: string) => ({
  id, login, globalRating: 1500, ratings: [], country: "", clan: "", avatarUrl: "", avatarTooltip: "",
});

function MenuOn({ names }: { names: string[] }) {
  const { openPlayerMenu, playerMenu } = usePlayerMenu();
  return (
    <>
      {names.map((name) => (
        <button key={name} type="button" onContextMenu={(event) => openPlayerMenu(name, event)}>{name}</button>
      ))}
      {playerMenu}
    </>
  );
}

describe("a build without play", () => {
  it("has no Play tab, and a way that still leads there opens the first tab", () => {
    expect(playIsOffered()).toBe(false);
    expect(tabsForMode("account")).toEqual(TAB_ORDER.filter((tab) => tab !== "play"));
    expect(tabsForMode("account")).toContain("replays");
    expect(openTabForMode("account", "play")).toBe("news");
    expect(openTabForMode("account", "replays")).toBe("replays");
  });

  it("offers no join, invite or kick in a player's menu, and still offers to watch", () => {
    seedStore((state) => ({
      ...state,
      auth: { ...state.auth, player: { id: 1, name: "Me", roles: [] } },
      social: { ...state.social, players: [player(41, "Host1"), player(42, "Live1")] },
      lobby: {
        ...state.lobby,
        games: [game(7, "Host1", ["Host1"])],
        liveGames: [game(8, "Live1", ["Live1"])],
        party: { ownerId: 1, members: [{ playerId: 1, name: "Me", factions: [] }, { playerId: 41, name: "Host1", factions: [] }] },
      },
    }));
    render(<MenuOn names={["Host1", "Live1"]} />);

    fireEvent.contextMenu(screen.getByRole("button", { name: "Host1" }));
    expect(screen.queryByText(en["chat.menu.joinGame"])).toBeNull();
    expect(screen.queryByText(en["chat.menu.kickFromParty"])).toBeNull();

    fireEvent.contextMenu(screen.getByRole("button", { name: "Live1" }));
    expect(screen.queryByText(en["chat.menu.inviteToParty"])).toBeNull();
    expect(screen.getByText(en["chat.menu.watchLive"])).toBeTruthy();
  });

  it("shows a chat link to join as text, and a link to a live replay as a link", () => {
    const parts = renderBody(
      "join fafgame://127.0.0.1/Ada?uid=7&map=scmp_009&mod=faf or watch faflive://127.0.0.1/8/Ada.SCFAreplay?map=scmp_009&mod=faf",
      "Me",
      "",
      () => undefined,
    );
    render(<p>{parts}</p>);
    const links = screen.getAllByRole("button");
    expect(links).toHaveLength(1);
    expect(links[0].getAttribute("title")).toBe(en["chat.link.watchLive"]);
    expect(screen.getByText(/fafgame:\/\/127\.0\.0\.1\/Ada/)).toBeTruthy();
  });
});

describe("a build that plays", () => {
  it("keeps the Play tab and the player menu's join and invite", () => {
    setPlayOffered(true);
    expect(tabsForMode("account")).toEqual(TAB_ORDER);
    seedStore((state) => ({
      ...state,
      social: { ...state.social, players: [player(41, "Host1")] },
      lobby: { ...state.lobby, games: [game(7, "Host1", ["Host1"])] },
    }));
    render(<MenuOn names={["Host1"]} />);
    fireEvent.contextMenu(screen.getByRole("button", { name: "Host1" }));
    expect(screen.getByText(en["chat.menu.joinGame"])).toBeTruthy();
    expect(screen.getByText(en["chat.menu.inviteToParty"])).toBeTruthy();
  });
});
