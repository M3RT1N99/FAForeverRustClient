// A pick list in view as it opens, for the popovers that share `MultiSelect`'s
// rules: `MultiSelect`, and the replay search's friends and saved searches.

import { useLayoutEffect, type RefObject } from "react";

/**
 * When `open` turns true, scroll `list` into view as far as it fits, from its
 * trigger down where the two share a box, if the list is drawn in the flow.
 *
 * In the phone's filter tray the lists are in the tray's flow
 * (online-replays.css), and one opened at the foot of the tray's short window
 * opened out of sight, as if the press had done nothing. Everywhere else a
 * list hangs over the page under the trigger just pressed and is left there:
 * scrolled to, one that ran past the side of the page would have scrolled the
 * page sideways, since `.content` hides its horizontal overflow and a script
 * can still scroll it.
 */
export function useRevealOnOpen(list: RefObject<HTMLElement | null>, open: boolean) {
  useLayoutEffect(() => {
    const shown = list.current;
    if (!open || !shown || getComputedStyle(shown).position !== "static") return;
    // The trigger's and the list's box, unless the tray took it apart to give
    // the list a line of its own.
    const holder = shown.parentElement;
    const target = holder && getComputedStyle(holder).display !== "contents" ? holder : shown;
    target.scrollIntoView({ block: "nearest" });
  }, [open, list]);
}
