// @vitest-environment happy-dom
//
// The shell on a phone lying down: the rail with a menu in the brand's
// place, the drawer that menu opens, and the ways it closes (a tap beside
// it, Escape, a place chosen in it, and Android's Back), with the focus
// going into it and coming back out. A desktop window of the same size with a
// mouse keeps the shell it had.

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { seedStore, sentCommands } from "../../testing/mounted";
import { PHONE_LYING_DOWN, setScreen, SHORT_DESKTOP } from "../../testing/screens";
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

const ANDROID =
  "Mozilla/5.0 (Linux; Android 16; SM-S908B; wv) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Mobile Safari/537.36";

function mountShell() {
  seedStore((state) => ({
    ...state,
    auth: { ...state.auth, status: "loggedIn", mode: "offline", player: null },
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

  it("keeps the rail and puts a menu where the brand was", () => {
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

describe("Android's Back", () => {
  beforeEach(() => {
    setScreen(PHONE_LYING_DOWN);
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(ANDROID);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("closes the open drawer instead of leaving the app", async () => {
    const shell = mountShell();
    const before = window.history.length;
    await userEvent.setup().click(menu());
    expect(window.history.length).toBe(before + 1);
    window.history.back();
    await waitFor(() => expect(shell.classList).not.toContain("is-drawer-open"));
    expect(document.activeElement).toBe(menu());
  });

  it("takes its history entry back off when the drawer is closed another way", async () => {
    const shell = mountShell();
    const entry = () => (window.history.state as Record<string, unknown> | null)?.fafSystemBack;
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
  it("keeps the brand, the sidebar and its resize handle for a mouse in a window as short as a phone", () => {
    setScreen(SHORT_DESKTOP);
    const shell = mountShell();
    expect(shell.classList).not.toContain("is-sidebar-rail");
    expect(shell.style.getPropertyValue("--sidebar-width")).not.toBe("64px");
    expect(screen.getByText("FAForever")).toBeTruthy();
    expect(screen.getByRole("separator")).toBeTruthy();
    expect(screen.queryByRole("button", { name: en["shell.sidebar.menu"] })).toBeNull();
    expect(scrim()).toBeNull();
  });
});
