// Android's Back for something open on the page, such as the shell's drawer.
//
// wry answers Back by going back in the webview's history while it can, and
// by handing it to Android, which leaves the app, once it cannot. So while
// something is open it holds one history entry of its own: Back takes that
// entry off, and the page closes what is open instead of leaving. Closed any
// other way (a tap beside it, Escape, a choice made in it), it takes its
// entry off itself, so the next Back leaves the app as before.
//
// Android only. A desktop webview has no Back of this kind; its mouse's back
// button would go through the same history.

import { useEffect, useRef } from "react";
import { runsOnAndroid } from "../platform";

/** Marks the history entries this hook adds, apart from any other state. */
const STATE_KEY = "fafSystemBack";
let entries = 0;

/**
 * While `open`, Android's Back runs `onBack` instead of leaving the app.
 * `onBack` is expected to close what is open; the latest one is always used.
 */
export function useSystemBack(open: boolean, onBack: () => void): void {
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;

  useEffect(() => {
    if (!open || !runsOnAndroid()) return undefined;
    entries += 1;
    const entry = entries;
    window.history.pushState({ [STATE_KEY]: entry }, "");
    const ours = () => (window.history.state as Record<string, unknown> | null)?.[STATE_KEY] === entry;
    let taken = false;
    // Our entry is gone from the top: Back took it. A popstate that leaves it
    // on top belongs to an entry pushed after ours.
    const popped = () => {
      if (ours()) return;
      taken = true;
      onBackRef.current();
    };
    window.addEventListener("popstate", popped);
    return () => {
      window.removeEventListener("popstate", popped);
      if (!taken && ours()) window.history.back();
    };
  }, [open]);
}
