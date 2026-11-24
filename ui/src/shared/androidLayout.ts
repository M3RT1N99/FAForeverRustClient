// How the page is laid out and drawn on Android.
//
// The webview there has no zoom of its own (wry's does nothing). Its wide
// viewport (MainActivity) lays the page out at the width the viewport names
// and draws it at the scale the viewport names, so those two set how large
// everything is drawn.
//
// The page is drawn at 100 %, as the phone's own apps and FAF's web client
// are, times the system's font size and the player's interface scale: at the
// default font one CSS px is one dp, and the body's 14 px is 14 dp. It used to
// be drawn smaller instead, until the desktop's smallest window (680 x 480)
// fit, and a phone lying down drew it at 0.76: body text at 10.6 dp and a
// replay card's facts at 8.3. A phone lying down is a short window, and the
// stylesheets have rules of their own for a short window on a touch screen,
// as they have for a narrow desktop window. A tablet lays the page out at its
// own size, as before.

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
}

/**
 * The layout for a webview `webview` CSS px large at the player's interface
 * scale (100 for 100 %) and the system's font scale (1 at the default size).
 * The width is whole pixels, and the scale the one that fills the webview with
 * it exactly.
 */
export function androidLayout(webview: Size, interfaceScale: number, fontScale: number): AndroidLayout {
  const width = Math.round((webview.width * 100) / (fontScale * interfaceScale));
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
