import { describe, expect, it } from "vitest";
import {
  MAX_LAYOUT_WIDTH,
  MIN_DRAW_SCALE,
  MIN_LAYOUT_WIDTH,
  androidLayout,
  webviewSizeToLayOut,
} from "./androidLayout";
import { liesDownAsAPhone } from "./phoneLayout";

/** The test emulator's webview lying down (2400 x 1080 at 420 dpi, less the bars and the cutout). */
const phone = { width: 866, height: 364 };
/** A tablet lying down, with room for the desktop layout. */
const tablet = { width: 1280, height: 752 };
/** The phone upright, split screen: its half of the screen above the other app's. */
const splitPane = { width: 412, height: 560 };

/** The height a layout `width` wide is on a webview of the `webview` size. */
const layoutHeight = (webview: { width: number; height: number }, width: number) =>
  Math.round((webview.height * width) / webview.width);

describe("androidLayout on a phone lying down", () => {
  it("draws it at 100 %, laid out at its own size", () => {
    expect(androidLayout(phone, 100)).toEqual({ width: 866, scale: 1, phone: true });
  });

  it("lays out the S22 Ultra in HD+ at its own size, which the menu and the cards are made for", () => {
    // 1544 x 720 at 300 dpi, less the cutout, the status bar and the navigation bar.
    expect(androidLayout({ width: 749, height: 354 }, 100)).toEqual({ width: 749, scale: 1, phone: true });
  });

  it("puts the player's interface scale on as a zoom of the whole page", () => {
    const layout = androidLayout(phone, 125);
    expect(layout.width).toBe(Math.round(phone.width / 1.25));
    expect(layout.scale).toBeCloseTo(1.25, 2);
    expect(layout.phone).toBe(true);
  });
});

describe("androidLayout on a window the phone's rules do not cover", () => {
  it("draws a split-screen pane upright no smaller than the floor, laid out wider than it is", () => {
    const layout = androidLayout(splitPane, 100);
    expect(layout.scale).toBeCloseTo(MIN_DRAW_SCALE, 3);
    expect(layout.width).toBe(Math.round(splitPane.width / MIN_DRAW_SCALE));
    expect(layout.phone).toBe(false);
    expect(liesDownAsAPhone({ width: layout.width, height: layoutHeight(splitPane, layout.width) })).toBe(false);
  });

  it("fits the narrowest layout's width in a narrow window", () => {
    const upright = { width: 600, height: 900 };
    const layout = androidLayout(upright, 100);
    expect(layout.width).toBe(MIN_LAYOUT_WIDTH);
    expect(layout.scale).toBeGreaterThanOrEqual(MIN_DRAW_SCALE);
  });

  it("lays out at the webview's own size, at 100 %, where the smallest window fits", () => {
    expect(androidLayout({ width: 1000, height: 700 }, 100)).toEqual({ width: 1000, scale: 1, phone: false });
  });

  it("keeps the desktop's first window as the widest layout, drawn larger on a wider screen", () => {
    const layout = androidLayout(tablet, 100);
    expect(layout.width).toBe(MAX_LAYOUT_WIDTH);
    expect(layout.scale).toBeGreaterThan(1);
  });

  it("gives way to the phone's rules where the interface scale leaves a window lying down and short", () => {
    // 500 x 350 at twice the size: the phone's layout, at 100 % times the interface scale.
    expect(androidLayout({ width: 1000, height: 700 }, 200)).toEqual({ width: 500, scale: 2, phone: true });
  });

  it("puts the player's interface scale on top", () => {
    expect(androidLayout(tablet, 150).width).toBe(Math.round((MAX_LAYOUT_WIDTH * 100) / 150));
  });
});

describe("androidLayout", () => {
  it.each([
    [phone, 100],
    [phone, 150],
    [tablet, 90],
    [splitPane, 125],
    [{ width: 785, height: 340 }, 110],
  ])("fills the webview's width exactly with the whole-pixel layout (%o at %i %%)", (webview, ui) => {
    const layout = androidLayout(webview, ui);
    expect(Number.isInteger(layout.width)).toBe(true);
    expect(layout.width * layout.scale).toBeCloseTo(webview.width, 6);
  });
});

describe("webviewSizeToLayOut", () => {
  it("takes the first size it is given", () => {
    expect(webviewSizeToLayOut(null, phone, false)).toEqual(phone);
  });

  it("keeps the size while the keyboard shortens the webview for a field", () => {
    expect(webviewSizeToLayOut(phone, { width: phone.width, height: 150 }, true)).toBe(phone);
  });

  it("takes a shorter webview without a field in focus, the system bars' room arriving late", () => {
    const shorter = { width: phone.width, height: 330 };
    expect(webviewSizeToLayOut(phone, shorter, false)).toBe(shorter);
  });

  it("takes a taller webview at the same width, the keyboard gone", () => {
    const taller = { width: phone.width, height: 400 };
    expect(webviewSizeToLayOut(phone, taller, true)).toBe(taller);
  });

  it("takes any height at another width, a phone turned round", () => {
    const upright = { width: 412, height: 800 };
    expect(webviewSizeToLayOut(phone, upright, true)).toBe(upright);
    expect(webviewSizeToLayOut(upright, phone, true)).toBe(phone);
  });
});
