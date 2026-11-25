// The page's viewport on Android, from `shared/androidLayout`, and the line the
// client log gets for each layout the page takes.
//
// The line is there because a screenshot from a phone does not say which
// screen density, font size or interface scale drew it, and the phone's
// settings are not in the logs otherwise. It names the screen, the webview,
// the system's font size and the text zoom MainActivity made of it, the
// interface scale and the layout chosen, and nothing about the player.

import { native } from "../ipc/native";
import { type Size, androidLayout, screenAround, webviewSizeToLayOut } from "./androidLayout";

/** The webview size the page was last laid out for. */
let laidOutFor: Size | null = null;

/**
 * The webview's own size in CSS px at 100 %: the screen less what the system
 * keeps beside it (a camera cutout, the status and navigation bars), which
 * `screen` still counts. The visual viewport is the laid-out size drawn at its
 * scale, so their product is the webview's size whatever the layout is. While
 * the keyboard is up it is the size from before (`webviewSizeToLayOut`).
 */
function webviewSize(): Size {
  const viewport = window.visualViewport;
  const now =
    viewport && viewport.scale > 0
      ? { width: viewport.width * viewport.scale, height: viewport.height * viewport.scale }
      : { width: window.screen.width, height: window.screen.height };
  const focused = document.activeElement;
  const typing =
    focused instanceof HTMLInputElement ||
    focused instanceof HTMLTextAreaElement ||
    (focused instanceof HTMLElement && focused.isContentEditable);
  laidOutFor = webviewSizeToLayOut(laidOutFor, now, typing);
  return laidOutFor;
}

/**
 * What MainActivity handed over before the page loaded: the system's font
 * scale (1 at the default size) and the webview's text zoom it set from it,
 * in percent. Either is null where it was not handed over, as on a webview
 * too old for a document-start script.
 */
function handedOver(name: "fafAndroidFontScale" | "fafAndroidTextZoom"): number | null {
  const value: unknown = (window as unknown as Record<string, unknown>)[name];
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

/**
 * Lay the page out for the webview as it is now, at the player's interface
 * scale (100 for 100 %), and log the layout when it is a new one. The scale
 * is held (its minimum and maximum), as the webview fits a page to its width
 * only when it loads. Set only when it changes: a new width resizes the page,
 * and that resize comes back here and leaves it as it is.
 */
export function applyAndroidLayout(interfaceScale: number): void {
  const webview = webviewSize();
  const layout = androidLayout(webview, interfaceScale);
  const drawn = layout.scale.toFixed(4);
  const content = `width=${layout.width}, initial-scale=${drawn}, minimum-scale=${drawn}, maximum-scale=${drawn}`;
  const viewport = document.querySelector('meta[name="viewport"]');
  if (!viewport || viewport.getAttribute("content") === content) return;
  viewport.setAttribute("content", content);
  // The screen the layout was taken in, which the webview may still report
  // turned the way it was before (`screenAround`).
  const around = screenAround(webview, { width: window.screen.width, height: window.screen.height });
  native.reportAndroidLayout({
    screenWidth: around.width,
    screenHeight: around.height,
    devicePixelRatio: window.devicePixelRatio,
    webviewWidth: webview.width,
    webviewHeight: webview.height,
    innerWidth: window.innerWidth,
    innerHeight: window.innerHeight,
    fontScale: handedOver("fafAndroidFontScale"),
    textZoom: handedOver("fafAndroidTextZoom"),
    interfaceScale,
    layoutWidth: layout.width,
    layoutScale: layout.scale,
    phone: layout.phone,
  });
}

/** Forget the webview size, for a test that starts on another screen. */
export function resetAndroidLayoutForTests(): void {
  laidOutFor = null;
}
