// @vitest-environment happy-dom
//
// What a finger can hit: on Android (`shared/phoneLayout`) the client's
// buttons, fields, dropdowns, switches, tabs and settings sections are at
// least 40 px high, whatever pointer the phone reports, and a button its
// feature makes taller keeps its height. The desktop keeps the sizes it had,
// in a window of the same size, with a mouse or a touch screen: the condition
// is the platform, never the width or the pointer.

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Button } from "./Button";
import { Modal } from "./Modal";
import { Select } from "./Select";
import { Switch } from "./Switch";
import {
  addStylesheets,
  PHONE_LYING_DOWN,
  PHONE_WITH_A_PEN,
  setScreen,
  SHORT_DESKTOP,
  SHORT_TOUCH_DESKTOP,
  type Screen,
} from "../testing/screens";
import "../testing/mounted";

const STYLESHEETS = [
  "ui/src/design-system/tokens.css",
  "ui/src/styles.css",
  "ui/src/design-system/select.css",
  "ui/src/design-system/switch.css",
  "ui/src/design-system/search-panel.css",
  "ui/src/design-system/modal.css",
  "ui/src/features/nav/nav.css",
  "ui/src/features/settings/settings.css",
  "ui/src/features/replays/online-replays.css",
  "ui/src/features/auth/auth.css",
];

function Controls() {
  return (
    <>
      <Button>Sign in</Button>
      <Button variant="primary" className="search-panel-submit">Search</Button>
      <Select label="Mode" value="all" options={[{ value: "all", label: "All files" }]} onChange={vi.fn()} />
      <Switch checked={false} onChange={vi.fn()} label="Turn with the phone" />
      <input className="vault-input search-panel-control" type="search" aria-label="Player" />
      <button type="button" className="tab">Replays</button>
      <button type="button" className="settings-sidebar-item">Appearance</button>
      <Button variant="primary" className="replay-card-watch">Watch</Button>
      <Button variant="primary" className="login-button">Log in with FAF</Button>
    </>
  );
}

function mount(on: Screen) {
  setScreen(on);
  addStylesheets(...STYLESHEETS);
  render(<Controls />);
  const style = (element: HTMLElement) => getComputedStyle(element);
  return {
    signIn: style(screen.getByRole("button", { name: "Sign in" })),
    search: style(screen.getByRole("button", { name: "Search" })),
    select: style(screen.getByRole("combobox", { name: "Mode" })),
    switchLabel: style(screen.getByRole("checkbox", { name: "Turn with the phone" }).closest("label")!),
    field: style(screen.getByRole("searchbox", { name: "Player" })),
    tab: style(screen.getByRole("button", { name: "Replays" })),
    section: style(screen.getByRole("button", { name: "Appearance" })),
    watch: style(screen.getByRole("button", { name: "Watch" })),
    logIn: style(screen.getByRole("button", { name: "Log in with FAF" })),
  };
}

describe("touch targets", () => {
  it.each([
    ["a phone", PHONE_LYING_DOWN],
    ["a phone with its pen, which reports a fine pointer", PHONE_WITH_A_PEN],
  ] as const)("are at least 40 px high on %s", (_, on) => {
    const sizes = mount(on);
    expect(sizes.signIn.minHeight).toBe("40px");
    expect(sizes.search.height).toBe("40px");
    expect(sizes.select.minHeight).toBe("40px");
    expect(sizes.switchLabel.minHeight).toBe("40px");
    expect(sizes.switchLabel.minWidth).toBe("40px");
    expect(sizes.field.height).toBe("40px");
    expect(sizes.field.fontSize).toBe("16px");
    expect(sizes.tab.minHeight).toBe("40px");
    expect(sizes.section.minHeight).toBe("40px");
    // A card's Watch, which its feature draws at 36 px for a mouse.
    expect(sizes.watch.minHeight).toBe("40px");
  });

  it("leave a button its feature makes taller at its own height", () => {
    expect(mount(PHONE_LYING_DOWN).logIn.minHeight).toBe("44px");
  });

  it("are on Android at any size, a tablet's too", () => {
    expect(mount({ ...PHONE_LYING_DOWN, width: 1280, height: 800 }).signIn.minHeight).toBe("40px");
  });

  it.each([
    ["a mouse", SHORT_DESKTOP],
    ["a touch screen", SHORT_TOUCH_DESKTOP],
  ] as const)("keep the desktop's sizes for %s in the same short window", (_, on) => {
    const sizes = mount(on);
    expect(sizes.signIn.minHeight).toBe("32px");
    expect(sizes.search.height).toBe("34px");
    expect(sizes.select.minHeight).not.toBe("40px");
    expect(sizes.switchLabel.height).toBe("22px");
    expect(sizes.switchLabel.minHeight).not.toBe("40px");
    expect(sizes.field.height).toBe("34px");
    expect(sizes.field.fontSize).not.toBe("16px");
    expect(sizes.tab.minHeight).toBe("34px");
    expect(sizes.section.minHeight).toBe("34px");
    expect(sizes.watch.minHeight).toBe("36px");
    expect(sizes.logIn.minHeight).toBe("44px");
  });

  it.each([
    ["a phone", PHONE_LYING_DOWN, true],
    ["a phone with its pen", PHONE_WITH_A_PEN, true],
    ["a desktop with a mouse", SHORT_DESKTOP, false],
    ["a desktop's touch screen", SHORT_TOUCH_DESKTOP, false],
  ] as const)("size a dialog's close button for %s", (_, on, square) => {
    setScreen(on);
    addStylesheets(...STYLESHEETS);
    render(<Modal onClose={vi.fn()} ariaLabel="Replay"><p>Details</p></Modal>);
    const close = getComputedStyle(document.querySelector<HTMLElement>(".modal-close")!);
    expect(close.minHeight === "40px" && close.minWidth === "40px").toBe(square);
  });
});
