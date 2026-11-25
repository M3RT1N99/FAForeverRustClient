// Android's Back for a layer open on the page, such as the shell's drawer.
//
// wry answers Back by going back in the webview's history while it can, and
// by handing it to Android, which leaves the app, once it cannot. So while a
// layer is open it holds one history entry of its own: Back takes that entry
// off, and the page does what Escape does instead of leaving (`overlayStack`).
// It closes the layer on top, which is this one, or one opened over it, such
// as a dialog opened from the drawer. Then this layer stays open and holds an
// entry again, for the Back that closes it in turn. Closed any other way (a
// tap beside it, Escape, a choice made in it), it takes its entry off itself,
// so the next Back leaves the app as before.
//
// Android only. A desktop webview has no Back of this kind; its mouse's back
// button would go through the same history.

import { useEffect } from "react";
import { overlayStack, type OverlayLayer } from "../../design-system/overlayStack";
import { runsOnAndroid } from "../platform";

/** Marks the history entries this hook adds, apart from any other state. */
const STATE_KEY = "fafSystemBack";
let entries = 0;

/**
 * While `open`, Android's Back closes the topmost layer of the overlay stack,
 * as Escape does, instead of leaving the app, until it has closed `layer`.
 * `layer` is the one `useOverlayLayer` registers for the same `open`.
 */
export function useSystemBack(open: boolean, layer: OverlayLayer): void {
  useEffect(() => {
    if (!open || !runsOnAndroid()) return undefined;
    entries += 1;
    const entry = entries;
    const hold = () => window.history.pushState({ [STATE_KEY]: entry }, "");
    hold();
    const ours = () => (window.history.state as Record<string, unknown> | null)?.[STATE_KEY] === entry;
    let taken = false;
    // Our entry is gone from the top: Back took it. A popstate that leaves it
    // on top belongs to an entry pushed after ours.
    const popped = () => {
      if (ours()) return;
      const top = overlayStack.top() ?? layer;
      if (top === layer) taken = true;
      else hold();
      top.onEscape();
    };
    window.addEventListener("popstate", popped);
    return () => {
      window.removeEventListener("popstate", popped);
      if (!taken && ours()) window.history.back();
    };
  }, [open, layer]);
}
