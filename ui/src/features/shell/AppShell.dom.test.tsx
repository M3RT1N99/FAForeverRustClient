// @vitest-environment happy-dom
//
// The shell on a phone lying down: the rail with a menu in the brand's
// place, the drawer that menu opens, and the ways it closes (a tap beside
// it, Escape, a place chosen in it, and Android's Back), with the focus
// going into it and coming back out. A phone with a pen, which reports a fine
// pointer, has them too. A desktop window of the same size, with a mouse or a
// touch screen, keeps the shell it had.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { applyEvent, seedStore, sentCommands } from "../../testing/mounted";
import { PHONE_LYING_DOWN, PHONE_WITH_A_PEN, setScreen, SHORT_DESKTOP, SHORT_TOUCH_DESKTOP } from "../../testing/screens";
import { AppShell } from "./AppShell";

vi.mock("../../ipc/client");
// The tabs' own views are not what is tested here, and each one loads on
// demand: the shell is drawn with an empty page in their place.
vi.mock("../nav/tabs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../nav/tabs")>();
  const TABS = Object.fromEntries(
    Object.entries(actual.TABS).map(([id, tab]) => [id, { ...tab, Component: () => null }]),
  ) as unknown as typeof actual.TABS;
  return { ...actual, TABS };
});

failOnConsoleError();

function mountShell(player: { id: number; name: string } | null = null) {
  seedStore((state) => ({
    ...state,
    auth: {
      ...state.auth,
      status: "loggedIn",
      mode: player ? "account" : "offline",
      player: player && { ...player, roles: [] },
    },
    nav: { ...state.nav, activeTab: "replays" },
  }));
  render(<AppShell />);
  return document.querySelector<HTMLElement>(".app-shell")!;
}

const menu = () => screen.getByRole("button", { name: en["shell.sidebar.menu"] });
const tab = (name: string) => screen.getByRole("button", { name });
const scrim = () => document.querySelector<HTMLElement>(".drawer-scrim");

describe("the shell on a phone lying down", () => {
  beforeEach(() => setScreen(PHONE_LYING_DOWN));

  it.each([
    ["a finger", PHONE_LYING_DOWN],
    ["its pen, which reports a fine pointer", PHONE_WITH_A_PEN],
  ] as const)("keeps the rail and puts a menu where the brand was, for %s", (_, on) => {
    setScreen(on);
    const shell = mountShell();
    expect(shell.classList).toContain("is-sidebar-rail");
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("64px");
    expect(menu().getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("FAForever")).toBeNull();
    // The resize handle is a mouse's; the phone keeps the rail.
    expect(screen.queryByRole("separator")).toBeNull();
  });

  it("opens the sidebar as a drawer over the page and moves the focus into it", async () => {
    const shell = mountShell();
    await userEvent.setup().click(menu());
    expect(menu().getAttribute("aria-expanded")).toBe("true");
    expect(shell.classList).toContain("is-drawer-open");
    expect(shell.classList).not.toContain("is-sidebar-rail");
    expect(scrim()).not.toBeNull();
    expect(document.activeElement).toBe(tab(en["nav.tab.replays.label"]));
  });

  it("closes on a tap beside it and gives the focus back to the menu", async () => {
    const shell = mountShell();
    await userEvent.setup().click(menu());
    fireEvent.click(scrim()!);
    expect(shell.classList).not.toContain("is-drawer-open");
    expect(shell.classList).toContain("is-sidebar-rail");
    expect(scrim()).toBeNull();
    expect(document.activeElement).toBe(menu());
  });

  it("closes on Escape", async () => {
    const user = userEvent.setup();
    const shell = mountShell();
    await user.click(menu());
    await user.keyboard("{Escape}");
    expect(shell.classList).not.toContain("is-drawer-open");
    expect(document.activeElement).toBe(menu());
  });

  it("closes when a place is chosen in it, the one already open too", async () => {
    const user = userEvent.setup();
    const shell = mountShell();
    await user.click(menu());
    await user.click(tab(en["nav.tab.settings.label"]));
    expect(shell.classList).not.toContain("is-drawer-open");
    expect(sentCommands()).toContainEqual({ kind: "Nav", command: { type: "select", payload: { tab: "settings" } } });
    await user.click(menu());
    await user.click(tab(en["nav.tab.replays.label"]));
    expect(shell.classList).not.toContain("is-drawer-open");
  });

  it("keeps Tab inside the open drawer", async () => {
    const user = userEvent.setup();
    mountShell();
    await user.click(menu());
    const sidebar = document.querySelector<HTMLElement>(".sidebar")!;
    const controls = Array.from(sidebar.querySelectorAll<HTMLElement>("button:not([disabled])"));
    controls[controls.length - 1].focus();
    await user.tab();
    expect(document.activeElement).toBe(controls[0]);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(controls[controls.length - 1]);
  });

  it("has no drawer once the phone is turned upright, and it does not come back open", async () => {
    const shell = mountShell();
    await userEvent.setup().click(menu());
    setScreen({ ...PHONE_LYING_DOWN, width: 340, height: 785 });
    await waitFor(() => expect(screen.queryByRole("button", { name: en["shell.sidebar.menu"] })).toBeNull());
    expect(shell.classList).not.toContain("is-drawer-open");
    setScreen(PHONE_LYING_DOWN);
    await waitFor(() => expect(menu().getAttribute("aria-expanded")).toBe("false"));
    expect(shell.classList).not.toContain("is-drawer-open");
  });
});

/** The shell's own entry on top of the history, while the drawer holds one. */
const entry = () => (window.history.state as Record<string, unknown> | null)?.fafSystemBack;

describe("Android's Back", () => {
  beforeEach(() => setScreen(PHONE_LYING_DOWN));

  it("closes the open drawer instead of leaving the app", async () => {
    const shell = mountShell();
    await userEvent.setup().click(menu());
    expect(entry()).toBeDefined();
    window.history.back();
    await waitFor(() => expect(shell.classList).not.toContain("is-drawer-open"));
    expect(document.activeElement).toBe(menu());
  });

  it("closes a dialog opened from the drawer first, and the drawer on the Back after", async () => {
    const user = userEvent.setup();
    const shell = mountShell({ id: 7, name: "Mrauky" });
    await user.click(menu());
    await user.click(screen.getByRole("button", { name: en["shell.sidebar.openProfile"].replace("{name}", "Mrauky") }));
    expect(sentCommands()).toContainEqual({
      kind: "PlayerCard",
      command: { type: "open", payload: { playerId: 7, login: "Mrauky" } },
    });
    // The core answers by opening the card, over the drawer.
    applyEvent({ kind: "PlayerCard", event: { type: "loading", payload: { login: "Mrauky" } } });
    const card = screen.getByRole("dialog", { name: "Player card: Mrauky" });

    window.history.back();
    expect(sentCommands()).toContainEqual({ kind: "PlayerCard", command: { type: "close" } });
    expect(shell.classList).toContain("is-drawer-open");
    expect(document.activeElement).not.toBe(menu());
    // The drawer holds its Back again.
    expect(entry()).toBeDefined();
    applyEvent({ kind: "PlayerCard", event: { type: "closed" } });
    expect(card.isConnected).toBe(false);
    expect(shell.classList).toContain("is-drawer-open");

    window.history.back();
    await waitFor(() => expect(shell.classList).not.toContain("is-drawer-open"));
    expect(document.activeElement).toBe(menu());
  });

  it("takes its history entry back off when the drawer is closed another way", async () => {
    const shell = mountShell();
    await userEvent.setup().click(menu());
    expect(entry()).toBeDefined();
    fireEvent.click(scrim()!);
    expect(shell.classList).not.toContain("is-drawer-open");
    await waitFor(() => expect(entry()).toBeUndefined());
  });

  it("adds no history entry while the drawer is closed", () => {
    const before = window.history.length;
    mountShell();
    expect(window.history.length).toBe(before);
  });
});

describe("the shell on a desktop", () => {
  it.each([
    ["a mouse", SHORT_DESKTOP],
    ["a touch screen", SHORT_TOUCH_DESKTOP],
  ] as const)("keeps the brand, the sidebar and its resize handle for %s in a window as short as a phone", (_, on) => {
    setScreen(on);
    const shell = mountShell();
    expect(shell.classList).not.toContain("is-sidebar-rail");
    expect(shell.style.getPropertyValue("--sidebar-width")).not.toBe("64px");
    expect(screen.getByText("FAForever")).toBeTruthy();
    expect(screen.getByRole("separator")).toBeTruthy();
    expect(screen.queryByRole("button", { name: en["shell.sidebar.menu"] })).toBeNull();
    expect(scrim()).toBeNull();
  });
});
