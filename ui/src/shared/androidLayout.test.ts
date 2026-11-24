import { describe, expect, it } from "vitest";
import {
  MAX_LAYOUT_WIDTH,
  MIN_DRAW_SCALE,
  MIN_LAYOUT_HEIGHT,
  MIN_LAYOUT_WIDTH,
  androidLayout,
  webviewSizeToLayOut,
} from "./androidLayout";

/** The test emulator's webview lying down (2400 x 1080 at 420 dpi, less the bars and the cutout). */
const phone = { width: 866, height: 364 };

/** The height a layout `width` wide is on a webview of the `webview` size. */
const layoutHeight = (webview: { width: number; height: number }, width: number) =>
  Math.round((webview.height * width) / webview.width);

describe("androidLayout", () => {
  it("draws a phone lying down smaller, until the desktop's smallest window fits", () => {
    const layout = androidLayout(phone, 100);
    expect(layout.width).toBe(1142);
    expect(layoutHeight(phone, layout.width)).toBe(MIN_LAYOUT_HEIGHT);
    expect(layout.scale).toBeLessThan(1);
    expect(layout.scale * layout.width).toBeCloseTo(phone.width, 6);
  });

  it("lays out at the webview's own size, at 100 %, where the smallest window fits", () => {
    const tablet = { width: 1000, height: 700 };
    expect(androidLayout(tablet, 100)).toEqual({ width: 1000, scale: 1 });
  });

  it("keeps the desktop's first window as the widest layout, drawn larger on a wider screen", () => {
    const layout = androidLayout({ width: 1280, height: 800 }, 100);
    expect(layout.width).toBe(MAX_LAYOUT_WIDTH);
    expect(layout.scale).toBeGreaterThan(1);
  });

  it("lays out wider than the desktop's first window when the height needs it", () => {
    // Short and wide: the smallest window's height wins over the widest layout.
    const layout = androidLayout({ width: 1000, height: 400 }, 100);
    expect(layout.width).toBeGreaterThan(MAX_LAYOUT_WIDTH);
    expect(layoutHeight({ width: 1000, height: 400 }, layout.width)).toBe(MIN_LAYOUT_HEIGHT);
  });

  it("draws no smaller than the floor, where the page scrolls instead", () => {
    const short = { width: 700, height: 300 };
    const layout = androidLayout(short, 100);
    expect(layout.scale).toBeCloseTo(MIN_DRAW_SCALE, 3);
    expect(layoutHeight(short, layout.width)).toBeLessThan(MIN_LAYOUT_HEIGHT);
  });

  it("fits the narrowest layout's width as well as the smallest window's height", () => {
    const narrow = { width: 600, height: 900 };
    const layout = androidLayout(narrow, 100);
    expect(layout.width).toBe(MIN_LAYOUT_WIDTH);
    expect(layout.scale).toBeGreaterThanOrEqual(MIN_DRAW_SCALE);
  });

  it("puts the player's interface scale on top", () => {
    expect(androidLayout(phone, 125).width).toBe(Math.round((1142 * 100) / 125));
    expect(androidLayout({ width: 1280, height: 800 }, 150).width).toBe(Math.round((MAX_LAYOUT_WIDTH * 100) / 150));
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
