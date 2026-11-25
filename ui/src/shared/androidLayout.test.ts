import { describe, expect, it } from "vitest";
import {
  MAX_LAYOUT_WIDTH,
  MIN_DRAW_SCALE,
  MIN_LAYOUT_HEIGHT,
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
    expect(androidLayout(phone, 100, 1)).toEqual({ width: 866, scale: 1, phone: true });
  });

  it.each([1.15, 1.3])("draws all of the page larger at the system's font size %s", (font) => {
    const layout = androidLayout(phone, 100, font);
    expect(layout.width).toBe(Math.round(phone.width / font));
    expect(layout.scale).toBeCloseTo(font, 2);
  });

  it("draws it smaller at a smaller font size, with no floor", () => {
    expect(androidLayout(phone, 100, 0.85).scale).toBeCloseTo(0.85, 2);
  });

  it("puts the player's interface scale on top of the font size", () => {
    expect(androidLayout(phone, 125, 1).width).toBe(Math.round(phone.width / 1.25));
    const both = androidLayout(phone, 125, 1.15);
    expect(both.width).toBe(Math.round(phone.width / (1.25 * 1.15)));
    expect(both.scale).toBeCloseTo(1.25 * 1.15, 2);
  });
});

describe("androidLayout on a window the phone's rules do not cover", () => {
  it("draws a split-screen pane upright no smaller than the floor, laid out wider than it is", () => {
    const layout = androidLayout(splitPane, 100, 1);
    expect(layout.scale).toBeCloseTo(MIN_DRAW_SCALE, 3);
    expect(layout.width).toBe(Math.round(splitPane.width / MIN_DRAW_SCALE));
    expect(liesDownAsAPhone({ width: layout.width, height: layoutHeight(splitPane, layout.width) })).toBe(false);
  });

  it("fits the narrowest layout's width in a narrow window", () => {
    const upright = { width: 600, height: 900 };
    const layout = androidLayout(upright, 100, 1);
    expect(layout.width).toBe(MIN_LAYOUT_WIDTH);
    expect(layout.scale).toBeGreaterThanOrEqual(MIN_DRAW_SCALE);
  });

  it("lays out at the webview's own size, at 100 %, where the smallest window fits", () => {
    expect(androidLayout({ width: 1000, height: 700 }, 100, 1)).toEqual({ width: 1000, scale: 1, phone: false });
  });

  it("keeps the desktop's first window as the widest layout, drawn larger on a wider screen", () => {
    const layout = androidLayout(tablet, 100, 1);
    expect(layout.width).toBe(MAX_LAYOUT_WIDTH);
    expect(layout.scale).toBeGreaterThan(1);
  });

  it("draws all of the page larger with the system's font size, where it still fits", () => {
    const window = { width: 1000, height: 700 };
    const layout = androidLayout(window, 100, 1.3);
    expect(layout.scale).toBeCloseTo(1.3, 2);
    expect(layoutHeight(window, layout.width)).toBeGreaterThanOrEqual(MIN_LAYOUT_HEIGHT);
  });

  it("never lets the font size draw the page past the narrowest layout", () => {
    expect(androidLayout({ width: 700, height: 1000 }, 100, 2).width).toBe(MIN_LAYOUT_WIDTH);
  });

  it("gives way to the phone's rules where the font size leaves a window lying down and short", () => {
    // 500 x 350 at twice the font: the phone's layout, at 100 % times the font.
    expect(androidLayout({ width: 1000, height: 700 }, 100, 2)).toEqual({ width: 500, scale: 2, phone: true });
  });

  it("puts the player's interface scale on top", () => {
    expect(androidLayout(tablet, 150, 1).width).toBe(Math.round((MAX_LAYOUT_WIDTH * 100) / 150));
  });
});

describe("androidLayout", () => {
  it.each([
    [phone, 100, 1.15],
    [phone, 150, 1.3],
    [tablet, 90, 1],
    [splitPane, 100, 1.3],
    [{ width: 785, height: 340 }, 110, 1.15],
  ])("fills the webview's width exactly with the whole-pixel layout (%o at %i %%, font %s)", (webview, ui, font) => {
    const layout = androidLayout(webview, ui, font);
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
