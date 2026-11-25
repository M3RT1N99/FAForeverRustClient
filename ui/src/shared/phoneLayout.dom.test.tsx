// @vitest-environment happy-dom
//
// The layout the page is in, for the code (`usePhoneLayout`) and for the
// stylesheets (`markTheLayout`), and again when the screen changes: a finger's
// on Android, whatever pointer the phone reports, and a phone's lying down;
// neither on a desktop, with a mouse or a touch screen.

import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { liesDownAsAPhone, PHONE_LYING_DOWN_LAYOUT, phoneLayout, TOUCH_LAYOUT, usePhoneLayout } from "./phoneLayout";
import "../testing/mounted";
import {
  DESKTOP,
  PHONE_LYING_DOWN,
  PHONE_WITH_A_PEN,
  setScreen,
  SHORT_DESKTOP,
  SHORT_TOUCH_DESKTOP,
  type Screen,
} from "../testing/screens";

function Probe() {
  return <output>{usePhoneLayout() ? "phone" : "other"}</output>;
}

const answer = () => screen.getByRole("status").textContent;
const root = () => document.documentElement;

const UPRIGHT: Screen = { ...PHONE_LYING_DOWN, width: 340, height: 785 };

const SCREENS: [string, Screen, { touch: boolean; phone: boolean }][] = [
  ["a phone lying down", PHONE_LYING_DOWN, { touch: true, phone: true }],
  ["a phone lying down with its pen, which reports a fine pointer", PHONE_WITH_A_PEN, { touch: true, phone: true }],
  ["the test emulator lying down", { ...PHONE_LYING_DOWN, width: 866, height: 364 }, { touch: true, phone: true }],
  ["a phone upright", UPRIGHT, { touch: true, phone: false }],
  ["a tablet lying down", { ...PHONE_LYING_DOWN, width: 1280, height: 752 }, { touch: true, phone: false }],
  ["a desktop window as short, with a mouse", SHORT_DESKTOP, { touch: false, phone: false }],
  ["a desktop window as short, on a touch screen", SHORT_TOUCH_DESKTOP, { touch: false, phone: false }],
  ["a desktop window", DESKTOP, { touch: false, phone: false }],
];

describe("usePhoneLayout", () => {
  it.each(SCREENS)("says whether %s is laid out as a phone", (_, on, expected) => {
    setScreen(on);
    render(<Probe />);
    expect(answer()).toBe(expected.phone ? "phone" : "other");
  });

  it("follows the phone turned upright and back", async () => {
    setScreen(PHONE_LYING_DOWN);
    render(<Probe />);
    expect(answer()).toBe("phone");
    setScreen(UPRIGHT);
    await waitFor(() => expect(answer()).toBe("other"));
    setScreen(PHONE_LYING_DOWN);
    await waitFor(() => expect(answer()).toBe("phone"));
  });
});

describe("markTheLayout", () => {
  it.each(SCREENS)("marks the root of %s for the stylesheets", (_, on, expected) => {
    setScreen(on);
    expect(root().matches(TOUCH_LAYOUT)).toBe(expected.touch);
    expect(root().matches(PHONE_LYING_DOWN_LAYOUT)).toBe(expected.phone);
  });

  it("keeps the mark on the screen as the phone turns", () => {
    setScreen(PHONE_LYING_DOWN);
    // The window alone turns, as a phone does, and the marks follow its size.
    // The first resize brings happy-dom's listener up to the screen as it is
    // (`setScreen`).
    window.dispatchEvent(new Event("resize"));
    (window as unknown as { happyDOM: { setWindowSize: (size: object) => void } }).happyDOM.setWindowSize({
      width: UPRIGHT.width,
      height: UPRIGHT.height,
    });
    window.dispatchEvent(new Event("resize"));
    expect(root().matches(PHONE_LYING_DOWN_LAYOUT)).toBe(false);
    expect(root().matches(TOUCH_LAYOUT)).toBe(true);
  });
});

describe("liesDownAsAPhone", () => {
  it.each([
    [785, 340],
    [866, 364],
    [900, 479],
    [900, 480],
    [479, 479],
    [480, 479],
    [340, 785],
    [1280, 752],
  ])("says of %i x %i what the page's media query says", (width, height) => {
    setScreen({ ...PHONE_LYING_DOWN, width, height });
    expect(liesDownAsAPhone({ width, height })).toBe(phoneLayout());
  });
});
