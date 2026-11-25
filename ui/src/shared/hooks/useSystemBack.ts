// Android's Back for something open on the page that Back should close before
// it leaves the app, such as the sidebar a phone's menu slid out.
//
// Tauri's app plugin answers Back by going back in the webview's history
// while it can, and by leaving the app once it cannot, unless the page listens
// for Back itself (`native.onBackButton`). So while it is open the page
// listens, and Back closes what is on top: a layer of the overlay stack first,
// as Escape does, such as a dialog opened from the sidebar, and once none is
// left the thing itself. The page keeps listening until that has closed,
// however that happens, and the next Back leaves the app as before.
//
// Not through the history, as this was first written: an entry the page adds
// for Back is one Chromium's Back skips when the page added it without a
// user's tap, which an entry added again after a Back is. On the emulator the
// second Back then left the app with the drawer it was written for open.
//
// Android only. A desktop webview has no Back of this kind.

import { useEffect, useRef } from "react";
import { overlayStack } from "../../design-system/overlayStack";
import { native } from "../../ipc/native";
import { runsOnAndroid } from "../platform";

/**
 * While `open`, Android's Back closes the topmost layer of the overlay stack,
 * as Escape does, and with none left calls `close`, instead of leaving the
 * app. The latest `close` is the one called.
 */
export function useSystemBack(open: boolean, close: () => void): void {
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    if (!open || !runsOnAndroid()) return undefined;
    // A Back already on its way when it closed finds nothing to do.
    let listening = true;
    const stop = native
      .onBackButton(() => {
        if (!listening) return;
        const top = overlayStack.top();
        if (top) top.onEscape();
        else closeRef.current();
      })
      .catch((error: unknown) => {
        // Back then leaves the app, as it did before the page listened.
        console.warn("Back cannot close what is open: no listener", error);
        return () => {};
      });
    return () => {
      listening = false;
      void stop.then((unlisten) => unlisten());
    };
  }, [open]);
}
