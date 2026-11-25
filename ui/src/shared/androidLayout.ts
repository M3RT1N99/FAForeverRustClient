// How the page is laid out and drawn on Android.
//
// The webview there has no zoom of its own (wry's does nothing). Its wide
// viewport (MainActivity) lays the page out at the width the viewport names
// and draws it at the scale the viewport names, so those two set how large
// everything is drawn.
//
// A phone lying down is drawn at 100 %, as Chrome draws FAF's web client and
// the phone draws its own apps: one CSS px is one dp, and the body's 14 px is
// 14 dp. The system's font size does not change that. It grows the text
// alone, as it does in Chrome, through the webview's own text zoom
// (MainActivity, up to a cap), and the layout keeps its size around it. The
// player's interface scale is a zoom of the whole page, on top. That is a
// short window, and the stylesheets have rules of their own for it
// (`shared/phoneLayout`), as they have for a narrow desktop window.
//
// Any other window, which those rules do not cover yet (a phone upright, a
// split-screen pane, a pop-up window, a tablet), keeps the fit it had: the
// makers' layout is made for a desktop window, which is never smaller than
// 560 x 480 CSS px (src-tauri/src/window.rs), so the page is drawn at the
// largest scale, up to 100 %, at which its layout is still at least
// MIN_LAYOUT_WIDTH x MIN_LAYOUT_HEIGHT. A screen with room to spare lays the
// page out at its own size, at 100 %, and one wider than MAX_LAYOUT_WIDTH
// draws that layout larger. Never below MIN_DRAW_SCALE: a screen too small for
// the smallest window at that scale lays the page out narrower instead. The
// player's interface scale comes on top, and may go past the fit.

import { liesDownAsAPhone } from "./phoneLayout";

/**
 * The narrowest width every page fits without scrolling sideways, measured on
 * every tab at 1100 down to 640 CSS px: the leaderboard's table is the first
 * to stick out, below 680. The shell turns its sidebar into a rail by itself
 * below 900.
 */
export const MIN_LAYOUT_WIDTH = 680;
/** The desktop window's smallest height (`min_inner_size` in window.rs). */
export const MIN_LAYOUT_HEIGHT = 480;
/** The desktop's first window; a wider screen draws this layout larger. */
export const MAX_LAYOUT_WIDTH = 1100;
/**
 * The smallest the page is drawn at outside a phone lying down, so that its
 * text stays readable: three quarters, what the phone tests before the fit to
 * the webview's width drew it at on a phone lying down (an 1100 px layout on
 * about 830 CSS px).
 */
export const MIN_DRAW_SCALE = 0.75;

/** A size in CSS px at 100 %. */
export interface Size {
  width: number;
  height: number;
}

/** The viewport the page is laid out and drawn with. */
export interface AndroidLayout {
  /** The width the page is laid out at, in CSS px. */
  width: number;
  /** The scale that layout is drawn at, which fills the webview's width. */
  scale: number;
  /** Laid out as a phone lying down, at 100 %, rather than fitted. */
  phone: boolean;
}

/**
 * The layout for a webview `webview` CSS px large at the player's interface
 * scale (100 for 100 %). The width is whole pixels, and the scale the one that
 * fills the webview with it exactly. The system's font size is not asked: it
 * grows the text, not the layout.
 */
export function androidLayout(webview: Size, interfaceScale: number): AndroidLayout {
  const full = layoutAt(webview, 1, interfaceScale);
  if (liesDownAsAPhone({ width: full.width, height: webview.height / full.scale })) return { ...full, phone: true };
  const fits = Math.min(webview.width / MIN_LAYOUT_WIDTH, webview.height / MIN_LAYOUT_HEIGHT);
  const preferred = Math.max(1, webview.width / MAX_LAYOUT_WIDTH);
  const fitted = layoutAt(webview, Math.max(MIN_DRAW_SCALE, Math.min(preferred, fits)), interfaceScale);
  return { ...fitted, phone: false };
}

/** The layout drawn at `scale` times the interface scale across the webview. */
function layoutAt(webview: Size, scale: number, interfaceScale: number): Omit<AndroidLayout, "phone"> {
  const width = Math.round((webview.width * 100) / (scale * interfaceScale));
  return { width, scale: webview.width / width };
}

/**
 * The webview size to lay the page out for, from the last one and the one
 * measured now. The keyboard shortens the webview while it is up (MainActivity
 * keeps the page above it), and the page is not drawn smaller for it: a lower
 * height at the same width while a field has the focus (`typing`) is the
 * keyboard's, and the last size stays. Without a field in focus it is the
 * webview's own, such as the system bars' room arriving after the first
 * layout.
 */
export function webviewSizeToLayOut(last: Size | null, now: Size, typing: boolean): Size {
  return typing && last && Math.abs(now.width - last.width) < 1 && now.height < last.height ? last : now;
}
