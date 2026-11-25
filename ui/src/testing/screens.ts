// The screens a layout test runs on, for `*.dom.test.tsx` files: a phone lying
// down, the same short window on a desktop with a mouse, and so on, with the
// client's own stylesheets applied so `getComputedStyle` answers as the page
// would.
//
// The page's root is marked with its layout as the client marks it
// (`markTheLayout`), from the user agent and the window's size. happy-dom
// answers the size queries from the window's size and `(pointer: coarse)` from
// `navigator.maxTouchPoints`, which is set too, so a test can show that the
// pointer changes nothing. happy-dom caches computed styles: set the screen
// first, then add the stylesheets and render.

import { readFileSync } from "node:fs";
import { afterEach } from "vitest";
import { markTheLayout } from "../shared/phoneLayout";

export interface Screen {
  width: number;
  height: number;
  /** Android's webview, which its user agent names. */
  android: boolean;
  /**
   * What the primary pointer reports: `coarse` for a finger, `fine` for a
   * mouse, a touchpad or a stylus, which Chromium on Android reports as soon
   * as one of them is there.
   */
  pointer: "coarse" | "fine";
}

/**
 * The S22 Ultra's webview lying down in HD+ (1544 x 720 at 300 dpi), less the
 * camera's cutout, the status bar and the navigation bar, as measured on the
 * tester's screenshots.
 */
export const PHONE_LYING_DOWN: Screen = { width: 749, height: 354, android: true, pointer: "coarse" };
/** The same phone with its S Pen, whose Chromium reports a fine pointer. */
export const PHONE_WITH_A_PEN: Screen = { ...PHONE_LYING_DOWN, pointer: "fine" };
/** The same window with a mouse: a desktop window this short at a large interface scale. */
export const SHORT_DESKTOP: Screen = { ...PHONE_LYING_DOWN, android: false, pointer: "fine" };
/** The same desktop window on a touch screen. */
export const SHORT_TOUCH_DESKTOP: Screen = { ...SHORT_DESKTOP, pointer: "coarse" };
/** A desktop window as the client first opens it. */
export const DESKTOP: Screen = { width: 1100, height: 720, android: false, pointer: "fine" };

/** The test phone's webview, as Android names it in the user agent. */
export const ANDROID_USER_AGENT =
  "Mozilla/5.0 (Linux; Android 16; SM-S908B; wv) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Mobile Safari/537.36";

interface HappyDomWindow {
  happyDOM: { setWindowSize: (size: { width: number; height: number }) => void };
}

const initial = { width: window.innerWidth, height: window.innerHeight };

let unmark: (() => void) | null = null;

/**
 * Size the window, say what it runs on and what its pointer is, and mark the
 * root as the client would. Undone after each test.
 *
 * happy-dom's media query lists tell their listeners of a change only on the
 * window's `resize`, and each listener starts out believing its query does not
 * match, whatever the screen. So the window says it was resized before the
 * change, which brings every listener up to the screen as it is, and again
 * after it, which is the change a browser would report.
 */
export function setScreen(screen: Screen): void {
  window.dispatchEvent(new Event("resize"));
  (window as unknown as HappyDomWindow).happyDOM.setWindowSize({ width: screen.width, height: screen.height });
  Object.defineProperty(navigator, "maxTouchPoints", {
    configurable: true,
    get: () => (screen.pointer === "coarse" ? 5 : 0),
  });
  if (screen.android) {
    Object.defineProperty(navigator, "userAgent", { configurable: true, get: () => ANDROID_USER_AGENT });
  } else {
    Reflect.deleteProperty(navigator, "userAgent");
  }
  window.dispatchEvent(new Event("resize"));
  unmark?.();
  unmark = markTheLayout(document.documentElement);
}

/**
 * Apply the client's stylesheets, by path from the repository root, in the
 * order given. Their `@import`s are left out: a test names each sheet it
 * needs. Removed after each test.
 */
export function addStylesheets(...paths: string[]): void {
  for (const path of paths) {
    const style = document.createElement("style");
    style.dataset.testStylesheet = path;
    style.textContent = readFileSync(path, "utf8").replace(/@import[^;]*;/g, "");
    document.head.appendChild(style);
  }
}

afterEach(() => {
  unmark?.();
  unmark = null;
  document.head.querySelectorAll("style[data-test-stylesheet]").forEach((style) => style.remove());
  (window as unknown as HappyDomWindow).happyDOM.setWindowSize(initial);
  Reflect.deleteProperty(navigator, "maxTouchPoints");
  Reflect.deleteProperty(navigator, "userAgent");
});
