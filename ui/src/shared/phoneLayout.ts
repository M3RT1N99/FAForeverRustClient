// The layout for a finger and for a phone lying down, where a stylesheet
// cannot tell them alone.
//
// A phone lying down is a short window: about 749 x 354 CSS px on the S22
// Ultra in HD+ and 866 x 364 on the test emulator, drawn at 100 %
// (`shared/androidLayout`). Its controls are a finger's size, and the page has
// rules of its own for a window that short, such as a menu that slides the
// sidebar out and a search folded to one row.
//
// Neither is asked of the pointer. Chromium on Android reports `pointer: fine`
// as soon as a stylus, a mouse or a touchpad is there, and the S22 Ultra's S
// Pen is one: keyed on `(pointer: coarse)`, the phone the layout was made for
// got the desktop's, cut off at the right at a large font. The client knows
// it runs on Android without asking the pointer (`runsOnAndroid`), and that is
// the condition: every Android window is laid out for a finger, and one lying
// down and short as a phone. A desktop window keeps its layout at any size and
// zoom, with a mouse or a touch screen.
//
// The page's root carries the answer (`markTheLayout`), and the stylesheets
// key their rules on it as `TOUCH_LAYOUT` and `PHONE_LYING_DOWN_LAYOUT` say,
// which `phoneLayout.test.ts` holds them to. Where the layout needs more than
// a rule, the component asks `usePhoneLayout`.

import { useSyncExternalStore } from "react";
import { runsOnAndroid } from "./platform";

/** The page laid out for a finger, as the stylesheets select it on the root. */
export const TOUCH_LAYOUT = "html[data-touch]";

/** A phone lying down, as the stylesheets select it on the root. */
export const PHONE_LYING_DOWN_LAYOUT = 'html[data-phone="lying-down"]';

/**
 * The tallest a phone lying down is, in CSS px: below the desktop window's
 * smallest height at 100 % (window.rs).
 */
const PHONE_MAX_HEIGHT = 479;

/** `liesDownAsAPhone` as a media query, which says when the answer changes. */
const LYING_DOWN = `(orientation: landscape) and (max-height: ${PHONE_MAX_HEIGHT}px)`;

/** Whether the page is laid out for a finger: on Android, whatever its pointer. */
export function laidOutForTouch(): boolean {
  return runsOnAndroid();
}

/**
 * Whether a page laid out `size` large lies down as a phone does: wider than
 * high, and shorter than any desktop window. `shared/androidLayout` asks it of
 * a layout before it makes one, the page of the layout it is in.
 */
export function liesDownAsAPhone(size: { width: number; height: number }): boolean {
  return size.width > size.height && size.height <= PHONE_MAX_HEIGHT;
}

function lyingDownQuery(): MediaQueryList | null {
  return typeof window === "undefined" || typeof window.matchMedia !== "function"
    ? null
    : window.matchMedia(LYING_DOWN);
}

function subscribe(changed: () => void): () => void {
  const query = lyingDownQuery();
  query?.addEventListener("change", changed);
  return () => query?.removeEventListener("change", changed);
}

/** Whether the page is laid out for a phone lying down, now. */
export function phoneLayout(): boolean {
  return laidOutForTouch() && (lyingDownQuery()?.matches ?? false);
}

/**
 * Mark `root` with the layout the page is in, and keep it marked as the
 * window turns or changes size: `data-touch` for a finger and
 * `data-phone="lying-down"` for a phone lying down. Before the first render,
 * so nothing is drawn in the other layout first. Returns what stops it and
 * takes the marks off.
 */
export function markTheLayout(root: HTMLElement): () => void {
  const mark = () => {
    root.toggleAttribute("data-touch", laidOutForTouch());
    if (phoneLayout()) root.dataset.phone = "lying-down";
    else delete root.dataset.phone;
  };
  mark();
  const stop = subscribe(mark);
  return () => {
    stop();
    root.removeAttribute("data-touch");
    delete root.dataset.phone;
  };
}

const notOnAServer = () => false;

/**
 * Whether the page is laid out for a phone lying down. Follows the screen: a
 * phone turned upright, or a window made taller, renders again with `false`.
 */
export function usePhoneLayout(): boolean {
  return useSyncExternalStore(subscribe, phoneLayout, notOnAServer);
}
