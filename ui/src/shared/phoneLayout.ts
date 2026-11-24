// The layout for a phone lying down, where a stylesheet cannot do it alone.
//
// A phone lying down is a short window on a touch screen: about 785 x 340 CSS
// px on the S22 Ultra and 866 x 364 on the test emulator, drawn at 100 %
// (`shared/androidLayout`). A desktop window is never shorter than 480 CSS px
// at 100 % (window.rs), but at a large interface scale it can be, and a mouse
// there keeps the desktop's layout: the condition asks for a touch screen as
// well as the height. Touch on any platform gets it, not Android alone.
//
// The stylesheets write the same condition in their media queries. Where the
// layout needs more than a rule, such as a menu that opens the sidebar as a
// drawer or a search folded to one row, the component asks `usePhoneLayout`.

import { useSyncExternalStore } from "react";

/**
 * A phone lying down, as a media query. Every stylesheet writes it out the
 * same, which `phoneLayout.test.ts` holds them to.
 */
export const PHONE_LYING_DOWN = "(pointer: coarse) and (orientation: landscape) and (max-height: 479px)";

function phoneQuery(): MediaQueryList | null {
  return typeof window === "undefined" || typeof window.matchMedia !== "function"
    ? null
    : window.matchMedia(PHONE_LYING_DOWN);
}

function subscribe(changed: () => void): () => void {
  const query = phoneQuery();
  query?.addEventListener("change", changed);
  return () => query?.removeEventListener("change", changed);
}

const lyingDown = () => phoneQuery()?.matches ?? false;
const notOnAServer = () => false;

/**
 * Whether the page is laid out for a phone lying down. Follows the screen: a
 * phone turned upright, or a window made taller, renders again with `false`.
 */
export function usePhoneLayout(): boolean {
  return useSyncExternalStore(subscribe, lyingDown, notOnAServer);
}
