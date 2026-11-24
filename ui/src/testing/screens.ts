// The screens a layout test runs on, for `*.dom.test.tsx` files: a phone lying
// down, the same short window on a desktop with a mouse, and so on, with the
// client's own stylesheets applied so `getComputedStyle` answers as the page
// would.
//
// happy-dom answers `(pointer: coarse)` from `navigator.maxTouchPoints` and the
// size queries from the window's size, and it caches computed styles: set the
// screen first, then add the stylesheets and render.

import { readFileSync } from "node:fs";
import { afterEach } from "vitest";

export interface Screen {
  width: number;
  height: number;
  /** A touch screen, which is what `(pointer: coarse)` asks about. */
  touch: boolean;
}

/** The S22 Ultra's webview lying down at the default font, as the A2 proposal estimates it. */
export const PHONE_LYING_DOWN: Screen = { width: 785, height: 340, touch: true };
/** The same window with a mouse: a desktop window this short at a large interface scale. */
export const SHORT_DESKTOP: Screen = { ...PHONE_LYING_DOWN, touch: false };
/** A desktop window as the client first opens it. */
export const DESKTOP: Screen = { width: 1100, height: 720, touch: false };

interface HappyDomWindow {
  happyDOM: { setWindowSize: (size: { width: number; height: number }) => void };
}

const initial = { width: window.innerWidth, height: window.innerHeight };

/** Size the window and say whether it is a touch screen. Undone after each test. */
export function setScreen(screen: Screen): void {
  (window as unknown as HappyDomWindow).happyDOM.setWindowSize({ width: screen.width, height: screen.height });
  Object.defineProperty(navigator, "maxTouchPoints", { configurable: true, get: () => (screen.touch ? 5 : 0) });
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
  document.head.querySelectorAll("style[data-test-stylesheet]").forEach((style) => style.remove());
  (window as unknown as HappyDomWindow).happyDOM.setWindowSize(initial);
  Reflect.deleteProperty(navigator, "maxTouchPoints");
});
