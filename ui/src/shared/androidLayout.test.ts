import { describe, expect, it } from "vitest";
import { MAX_LAYOUT_WIDTH, MIN_DRAW_SCALE, MIN_LAYOUT_WIDTH, androidLayoutWidth } from "./androidLayout";

/** How large a screen `screenWidth` wide draws a layout `layoutWidth` wide. */
const drawn = (screenWidth: number, layoutWidth: number) => screenWidth / layoutWidth;

describe("androidLayoutWidth", () => {
  it("lays out at the screen's own width wherever every page fits it, drawn at 100 %", () => {
    // A phone lying down, about the S22 Ultra's width.
    expect(androidLayoutWidth(823, 100)).toBe(823);
    expect(androidLayoutWidth(MIN_LAYOUT_WIDTH, 100)).toBe(MIN_LAYOUT_WIDTH);
  });

  it("keeps the desktop's first window as the widest layout, drawn larger on a wider screen", () => {
    expect(androidLayoutWidth(1280, 100)).toBe(MAX_LAYOUT_WIDTH);
    expect(drawn(1280, androidLayoutWidth(1280, 100))).toBeGreaterThan(1);
  });

  it("draws the narrowest layout smaller on a narrower screen, down to the floor", () => {
    const screen = MIN_LAYOUT_WIDTH * 0.9;
    expect(androidLayoutWidth(screen, 100)).toBe(MIN_LAYOUT_WIDTH);
    expect(drawn(screen, MIN_LAYOUT_WIDTH)).toBeGreaterThanOrEqual(MIN_DRAW_SCALE);
  });

  it("scrolls sideways rather than drawing below the floor", () => {
    // A phone held upright: about 390 CSS px.
    const layout = androidLayoutWidth(390, 100);
    expect(layout).toBeLessThan(MIN_LAYOUT_WIDTH);
    expect(drawn(390, layout)).toBeCloseTo(MIN_DRAW_SCALE, 2);
  });

  it("puts the player's interface scale on top", () => {
    expect(androidLayoutWidth(823, 125)).toBe(Math.round((823 * 100) / 125));
    expect(androidLayoutWidth(1280, 150)).toBe(Math.round((MAX_LAYOUT_WIDTH * 100) / 150));
  });
});
