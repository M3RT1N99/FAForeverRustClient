import { describe, expect, it } from "vitest";
import { androidLayout, webviewSizeToLayOut } from "./androidLayout";

/** The test emulator's webview lying down (2400 x 1080 at 420 dpi, less the bars and the cutout). */
const phone = { width: 866, height: 364 };
/** A tablet lying down, with room for the desktop layout. */
const tablet = { width: 1280, height: 752 };

describe("androidLayout", () => {
  it("draws a phone lying down at 100 %, laid out at its own size", () => {
    expect(androidLayout(phone, 100, 1)).toEqual({ width: 866, scale: 1 });
  });

  it("lays a tablet out at its own size too, the desktop's first window or wider", () => {
    expect(androidLayout(tablet, 100, 1)).toEqual({ width: 1280, scale: 1 });
    expect(androidLayout({ width: 1000, height: 700 }, 100, 1)).toEqual({ width: 1000, scale: 1 });
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

  it.each([
    [phone, 100, 1.15],
    [phone, 150, 1.3],
    [tablet, 90, 1],
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
