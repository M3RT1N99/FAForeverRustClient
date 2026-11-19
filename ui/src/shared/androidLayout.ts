// The width the page is laid out at on Android.
//
// The webview there has no zoom of its own (wry's does nothing). Its wide
// viewport (MainActivity) draws the page's layout width across the screen
// instead, so the layout width sets how large everything is drawn.
//
// The makers' layout is responsive, so the page is laid out at the screen's
// own width wherever every page still fits it, and drawn at 100 %: from
// MIN_LAYOUT_WIDTH, the narrowest width every page fits without scrolling
// sideways, up to MAX_LAYOUT_WIDTH, the desktop's first window. A narrower
// screen (a small phone, a phone held upright) draws that narrowest layout
// smaller, but not below MIN_DRAW_SCALE: below that the page scrolls sideways
// instead of shrinking further. The player's interface scale comes on top.

/**
 * The narrowest width every page fits without scrolling sideways, measured on
 * every tab at 1100 down to 640 CSS px: the leaderboard's table is the first
 * to stick out, below 680. The shell turns its sidebar into a rail by itself
 * below 900.
 */
export const MIN_LAYOUT_WIDTH = 680;
/** The desktop's first window; a wider screen draws this layout larger. */
export const MAX_LAYOUT_WIDTH = 1100;
/** The smallest the page is drawn at to keep the narrowest layout whole. */
export const MIN_DRAW_SCALE = 0.85;

/**
 * The layout width for a screen `screenWidth` CSS px wide at the player's
 * interface scale (100 for 100 %).
 */
export function androidLayoutWidth(screenWidth: number, interfaceScale: number): number {
  const fitting = Math.min(MIN_LAYOUT_WIDTH, screenWidth / MIN_DRAW_SCALE);
  const base = Math.min(MAX_LAYOUT_WIDTH, Math.max(screenWidth, fitting));
  return Math.round((base * 100) / interfaceScale);
}
