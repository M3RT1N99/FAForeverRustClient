// @vitest-environment happy-dom
//
// Replays on a phone lying down. Both searches fold to the player field,
// Search and More filters, which opens all of the rest and counts what it
// hides; the cards go two to a row where two fit, in their compact form; the
// list closes its columns up; and a replay's detail keeps Watch beside the
// title. A desktop window as short, with a mouse, keeps each as it was.

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { VaultReplay } from "../../ipc/bindings";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import "../../testing/mounted";
import { addStylesheets, PHONE_LYING_DOWN, PHONE_WITH_A_PEN, setScreen, SHORT_DESKTOP, type Screen } from "../../testing/screens";
import { EMPTY_REPLAY_QUERY } from "../../shared/replayQuery";
import { LocalReplaySearch } from "./local/LocalReplaySearch";
import { EMPTY_LOCAL_REPLAY_QUERY } from "./local/localReplayQuery";
import { VaultSearch } from "./online/VaultSearch";
import { ReplayCard } from "./ReplayCard";
import { ReplayDetailPanel } from "./ReplayDetailPanel";
import { ReplayList, type ReplayListRow } from "./ReplayList";

vi.mock("../../ipc/client");

failOnConsoleError();

const STYLESHEETS = [
  "ui/src/design-system/tokens.css",
  "ui/src/styles.css",
  "ui/src/design-system/search-panel.css",
  "ui/src/design-system/modal.css",
  "ui/src/features/replays/online-replays.css",
  "ui/src/features/replays/replays.css",
];

function on(screenSize: Screen) {
  setScreen(screenSize);
  addStylesheets(...STYLESHEETS);
}

function mountLocalSearch() {
  render(
    <LocalReplaySearch
      initialQuery={{ ...EMPTY_LOCAL_REPLAY_QUERY, map: "artic", status: "complete" }}
      self="Mrauky"
      featuredMods={["faf"]}
      loading={false}
      busy={false}
      onSearch={vi.fn()}
      onRefresh={vi.fn()}
      onOpenFile={vi.fn()}
    />,
  );
  return document.querySelector<HTMLFormElement>("form.local-vault-search")!;
}

function mountVaultSearch() {
  render(
    <VaultSearch
      featuredMods={["faf"]}
      leaderboards={[]}
      self="Mrauky"
      friends={[]}
      initialQuery={{ ...EMPTY_REPLAY_QUERY, map: "seton", title: "theos" }}
      onSearch={vi.fn()}
    />,
  );
  return document.querySelector<HTMLFormElement>("form.online-vault-search")!;
}

const player = () => screen.getByRole("searchbox", { name: en["replays.search.player"] });
const map = () => screen.queryByRole("searchbox", { name: en["replays.search.map"] });
const newest = () => screen.queryByRole("button", { name: en["replays.search.preset.newest"] });
const toggle = () => screen.getByRole("button", { name: /filters/i });

describe.each([
  ["local", mountLocalSearch, 1 + 2],
  ["vault", mountVaultSearch, 1 + 1],
] as const)("the %s search on a phone lying down", (_, mount, hidden) => {
  it("is one row: the player field, Search, and More filters with the count of what it hides", () => {
    on(PHONE_LYING_DOWN);
    const form = mount();
    expect(form.classList).toContain("is-folded");
    const row = form.querySelector<HTMLElement>(".vault-search-primary")!;
    expect(within(row).getByRole("searchbox", { name: en["replays.search.player"] })).toBe(player());
    expect(within(row).getByRole("button", { name: /Search/ })).toBeTruthy();
    expect(within(row).getByRole("button", { name: /filters/i }).textContent).toContain(
      en["replays.search.moreFiltersCount"].replace("{count}", String(hidden)),
    );
    expect(map()).toBeNull();
    expect(newest()).toBeNull();
    // The label is still the field's name, for a screen reader, but takes no room.
    expect(getComputedStyle(row.querySelector(".search-panel-label")!).position).toBe("absolute");
    expect(getComputedStyle(row).flexWrap).toBe("nowrap");
  });

  it("opens all of the rest with More filters, and folds it again", async () => {
    on(PHONE_LYING_DOWN);
    const user = userEvent.setup();
    const form = mount();
    await user.click(toggle());
    expect(form.classList).not.toContain("is-folded");
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
    expect(map()).not.toBeNull();
    expect(newest()).not.toBeNull();
    // The advanced panel opens with it.
    expect(form.querySelector(".search-panel-advanced, .vault-search-advanced")).not.toBeNull();
    await user.click(toggle());
    expect(form.classList).toContain("is-folded");
    expect(map()).toBeNull();
  });

  it("keeps every row for a mouse in a desktop window as short", () => {
    on(SHORT_DESKTOP);
    const form = mount();
    expect(form.classList).not.toContain("is-folded");
    expect(map()).not.toBeNull();
    expect(newest()).not.toBeNull();
    expect(toggle().closest(".vault-search-presets")).not.toBeNull();
  });
});

const replay = (uid: number, title: string): VaultReplay => ({
  uid,
  title,
  map: "scmp_009",
  mapThumbnailUrl: "",
  modName: "faf",
  startTime: "2026-10-25T06:44:00Z",
  replayAvailable: true,
  durationSeconds: 2_699,
  gameDurationSeconds: null,
  teams: [
    { team: 1, players: [{ name: "Mrauky", faction: 1, rating: 0, outcome: "", score: null }] },
    { team: 2, players: [{ name: "Janicki", faction: 2, rating: 450, outcome: "", score: null }] },
  ],
  averageRating: 225,
  quality: null,
  reviewsAverage: null,
  reviewsCount: null,
  gameVersion: null,
});

function mountCards() {
  render(
    <div className="replay-grid">
      <ReplayCard replay={replay(1, "theos best buddies")} watched={false} onOpen={vi.fn()} onWatch={vi.fn()} />
      <ReplayCard replay={replay(2, "Youbakafoolish")} watched={false} onOpen={vi.fn()} onWatch={vi.fn()} />
    </div>,
  );
  const card = document.querySelector<HTMLElement>(".replay-card")!;
  return {
    grid: getComputedStyle(document.querySelector<HTMLElement>(".replay-grid")!),
    card: getComputedStyle(card),
    thumb: getComputedStyle(card.querySelector<HTMLElement>(".replay-card-thumb")!),
    facts: getComputedStyle(card.querySelector<HTMLElement>(".replay-meta-grid")!),
  };
}

describe("replay cards", () => {
  it.each([
    ["a phone lying down", PHONE_LYING_DOWN],
    ["a phone lying down with its pen, which reports a fine pointer", PHONE_WITH_A_PEN],
  ] as const)("go two to a row on %s where two fit, each in its compact form", (_, phone) => {
    on(phone);
    const { grid, card, thumb, facts } = mountCards();
    expect(grid.gridTemplateColumns).toBe("repeat(auto-fill, minmax(min(100%, 320px), 1fr))");
    expect(card.display).toBe("grid");
    expect(card.gridTemplateColumns).toBe("72px minmax(0, 1fr)");
    expect(thumb.width).toBe("72px");
    expect(facts.gridTemplateColumns).toBe("repeat(3, minmax(0, 1fr))");
    expect(facts.fontSize).toBe("12px");
  });

  it("keep the desktop's card for a mouse in a window as short", () => {
    on(SHORT_DESKTOP);
    const { card, thumb, facts } = mountCards();
    expect(card.display).toBe("flex");
    expect(thumb.width).toBe("128px");
    expect(facts.fontSize).toBe("11px");
  });
});

const row: ReplayListRow = {
  key: "a",
  mapName: "scmp_009",
  mapThumbnailUrl: "",
  game: { primary: "theos best buddies" },
  played: { primary: "06:44" },
  players: { primary: "4" },
  rating: { primary: "225" },
  mod: { primary: "faf" },
  duration: { primary: "44m 59s" },
  replay: { primary: "Complete", tone: "ok" },
};

describe("the replay list", () => {
  it.each([
    ["half the room between columns on a phone lying down", PHONE_LYING_DOWN, "8px"],
    ["the desktop's room for a mouse in a window as short", SHORT_DESKTOP, "16px"],
  ] as const)("keeps %s", (_, size, gap) => {
    on(size);
    render(<ReplayList groups={[{ label: "Oct 25, 2026", rows: [row] }]} footer={null} />);
    expect(getComputedStyle(document.querySelector<HTMLElement>(".replay-list-row")!).columnGap).toBe(gap);
  });
});

describe("a replay's detail", () => {
  it.each([
    ["keeps the hero compact with Watch beside the title on a phone lying down", PHONE_LYING_DOWN, "96px minmax(0, 1fr) auto"],
    ["keeps the desktop's hero for a mouse in a window as short", SHORT_DESKTOP, "148px minmax(0, 1fr) auto"],
  ] as const)("%s", (_, size, columns) => {
    on(size);
    render(<ReplayDetailPanel replay={replay(28010105, "theos best buddies")} busy={false} onClose={vi.fn()} onWatch={vi.fn()} />);
    const hero = getComputedStyle(document.querySelector<HTMLElement>(".replay-detail-hero")!);
    expect(hero.gridTemplateColumns).toBe(columns);
    // Taller than a touch target already, and kept so on a touch screen.
    expect(getComputedStyle(screen.getByRole("button", { name: en["replays.detail.watch"] })).minHeight).toBe("52px");
  });
});
