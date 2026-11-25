// @vitest-environment happy-dom
//
// The shell on a phone lying down: the rail with a menu in the brand's
// place, which slides the sidebar out with its labels beside the page,
// pushing the page over, and back in; and Android's Back, which closes a
// dialog over the page first and then takes the sidebar in. A phone with a
// pen, which reports a fine pointer, has them too. A desktop window of the
// same size, with a mouse or a touch screen, keeps the shell it had.

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { applyEvent, seedStore, sentCommands } from "../../testing/mounted";
import {
  addStylesheets,
  PHONE_LYING_DOWN,
  PHONE_WITH_A_PEN,
  setScreen,
  SHORT_DESKTOP,
  SHORT_TOUCH_DESKTOP,
} from "../../testing/screens";
import { AppShell } from "./AppShell";

vi.mock("../../ipc/client");
// Android's Back as Tauri's app plugin hands it to the page: to the listeners
// registered at the time, and to nobody once they are gone.
const back = vi.hoisted(() => ({ listeners: new Set<() => void>() }));
vi.mock("../../ipc/native", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../ipc/native")>();
  const onBackButton = (handler: () => void) => {
    back.listeners.add(handler);
    return Promise.resolve(() => void back.listeners.delete(handler));
  };
  return { ...actual, native: { ...actual.native, onBackButton } };
});
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

/** Android's Back, pressed once. */
const pressBack = () => act(() => back.listeners.forEach((listener) => listener()));
const menu = () => screen.getByRole("button", { name: en["shell.sidebar.menu"] });
const tab = (name: string) => screen.getByRole("button", { name });
const sidebar = () => document.querySelector<HTMLElement>(".sidebar")!;
/** A place's label in the sidebar, which the rail hides. */
const label = (name: string) => tab(name).querySelector("span")!;

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

  it("slides the sidebar out with its labels beside the page, which moves over for it", async () => {
    addStylesheets("ui/src/design-system/tokens.css", "ui/src/features/shell/shell.css", "ui/src/features/nav/nav.css");
    const shell = mountShell();
    expect(getComputedStyle(shell).gridTemplateColumns).toBe("64px minmax(0, 1fr)");
    expect(getComputedStyle(label(en["nav.tab.replays.label"])).display).toBe("none");
    await userEvent.setup().click(menu());
    expect(menu().getAttribute("aria-expanded")).toBe("true");
    expect(shell.classList).toContain("is-sidebar-out");
    expect(shell.classList).not.toContain("is-sidebar-rail");
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("224px");
    // A column of the page's grid, not a layer over the page.
    expect(getComputedStyle(shell).gridTemplateColumns).toBe("224px minmax(0, 1fr)");
    expect(getComputedStyle(sidebar()).position).not.toBe("fixed");
    expect(getComputedStyle(label(en["nav.tab.replays.label"])).display).toBe("inline");
    // The menu stays where it was pressed, for the press that takes it in.
    expect(getComputedStyle(menu()).width).toBe("48px");
    expect(getComputedStyle(menu()).alignSelf).toBe("flex-start");
  });

  it("is the page's sidebar for a screen reader, out or in, and leaves the focus on the menu", async () => {
    mountShell();
    await userEvent.setup().click(menu());
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("complementary")).toBe(sidebar());
    expect(sidebar().hasAttribute("aria-modal")).toBe(false);
    expect(document.activeElement).toBe(menu());
  });

  it("takes the sidebar in again with the menu", async () => {
    const user = userEvent.setup();
    const shell = mountShell();
    await user.click(menu());
    await user.click(menu());
    expect(menu().getAttribute("aria-expanded")).toBe("false");
    expect(shell.classList).toContain("is-sidebar-rail");
    expect(shell.classList).not.toContain("is-sidebar-out");
    expect(shell.style.getPropertyValue("--sidebar-width")).toBe("64px");
    expect(document.activeElement).toBe(menu());
  });

  it("stays out when a place is chosen in it, until the menu takes it in", async () => {
    const user = userEvent.setup();
    const shell = mountShell();
    await user.click(menu());
    await user.click(tab(en["nav.tab.settings.label"]));
    expect(sentCommands()).toContainEqual({ kind: "Nav", command: { type: "select", payload: { tab: "settings" } } });
    expect(shell.classList).toContain("is-sidebar-out");
  });

  it("has the rail once the phone is turned upright and back, the sidebar in", async () => {
    const shell = mountShell();
    await userEvent.setup().click(menu());
    setScreen({ ...PHONE_LYING_DOWN, width: 340, height: 785 });
    await waitFor(() => expect(screen.queryByRole("button", { name: en["shell.sidebar.menu"] })).toBeNull());
    expect(shell.classList).not.toContain("is-sidebar-out");
    setScreen(PHONE_LYING_DOWN);
    await waitFor(() => expect(menu().getAttribute("aria-expanded")).toBe("false"));
    expect(shell.classList).toContain("is-sidebar-rail");
  });
});

describe("Android's Back", () => {
  beforeEach(() => setScreen(PHONE_LYING_DOWN));

  it("takes the slid-out sidebar in instead of leaving the app", async () => {
    const shell = mountShell();
    await userEvent.setup().click(menu());
    await waitFor(() => expect(back.listeners.size).toBe(1));
    pressBack();
    expect(shell.classList).not.toContain("is-sidebar-out");
    expect(shell.classList).toContain("is-sidebar-rail");
    // In, it lets the next Back leave the app.
    await waitFor(() => expect(back.listeners.size).toBe(0));
  });

  it("closes a dialog over the page first, and takes the sidebar in on the Back after", async () => {
    const user = userEvent.setup();
    const shell = mountShell({ id: 7, name: "Mrauky" });
    await user.click(menu());
    await user.click(screen.getByRole("button", { name: en["shell.sidebar.openProfile"].replace("{name}", "Mrauky") }));
    expect(sentCommands()).toContainEqual({
      kind: "PlayerCard",
      command: { type: "open", payload: { playerId: 7, login: "Mrauky" } },
    });
    // The core answers by opening the card, over the page.
    applyEvent({ kind: "PlayerCard", event: { type: "loading", payload: { login: "Mrauky" } } });
    const card = screen.getByRole("dialog", { name: "Player card: Mrauky" });
    await waitFor(() => expect(back.listeners.size).toBe(1));

    pressBack();
    expect(sentCommands()).toContainEqual({ kind: "PlayerCard", command: { type: "close" } });
    expect(shell.classList).toContain("is-sidebar-out");
    applyEvent({ kind: "PlayerCard", event: { type: "closed" } });
    expect(card.isConnected).toBe(false);
    expect(shell.classList).toContain("is-sidebar-out");
    // Still listening, for the Back that takes the sidebar in.
    expect(back.listeners.size).toBe(1);

    pressBack();
    expect(shell.classList).not.toContain("is-sidebar-out");
    await waitFor(() => expect(back.listeners.size).toBe(0));
  });

  it("stops listening when the menu takes the sidebar in", async () => {
    const user = userEvent.setup();
    mountShell();
    await user.click(menu());
    await waitFor(() => expect(back.listeners.size).toBe(1));
    await user.click(menu());
    await waitFor(() => expect(back.listeners.size).toBe(0));
  });

  it("does not listen while the sidebar is in, so Back leaves the app", () => {
    mountShell();
    expect(back.listeners.size).toBe(0);
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
    expect(shell.classList).not.toContain("is-sidebar-out");
    expect(shell.style.getPropertyValue("--sidebar-width")).not.toBe("64px");
    expect(screen.getByText("FAForever")).toBeTruthy();
    expect(screen.getByRole("separator")).toBeTruthy();
    expect(screen.queryByRole("button", { name: en["shell.sidebar.menu"] })).toBeNull();
  });
});
