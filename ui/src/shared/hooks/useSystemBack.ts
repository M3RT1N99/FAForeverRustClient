// Android's Back for a layer open on the page, such as the shell's drawer.
//
// Tauri's app plugin answers Back by going back in the webview's history
// while it can, and by leaving the app once it cannot, unless the page listens
// for Back itself (`native.onBackButton`). So while a layer is open the page
// listens, and Back does what Escape does (`overlayStack`): it closes the
// layer on top, which is this one, or one opened over it, such as a dialog
// opened from the drawer. The page keeps listening until this layer has
// closed, however that happens, and the next Back leaves the app as before.
//
// Not through the history, as this was first written: an entry the page adds
// for Back is one Chromium's Back skips when the page added it without a
// user's tap, which an entry added again after a Back is. On the emulator the
// second Back then left the app with the drawer open.
//
// Android only. A desktop webview has no Back of this kind.

import { useEffect } from "react";
import { overlayStack, type OverlayLayer } from "../../design-system/overlayStack";
import { native } from "../../ipc/native";
import { runsOnAndroid } from "../platform";

/**
 * While `open`, Android's Back closes the topmost layer of the overlay stack,
 * as Escape does, instead of leaving the app, until it has closed `layer`.
 * `layer` is the one `useOverlayLayer` registers for the same `open`.
 */
export function useSystemBack(open: boolean, layer: OverlayLayer): void {
  useEffect(() => {
    if (!open || !runsOnAndroid()) return undefined;
    // A Back already on its way when the layer closed finds nothing to do.
    let listening = true;
    const stop = native
      .onBackButton(() => {
        if (listening) (overlayStack.top() ?? layer).onEscape();
      })
      .catch((error: unknown) => {
        // Back then leaves the app, as it did before the page listened.
        console.warn("Back cannot close the layer on top: no listener", error);
        return () => {};
      });
    return () => {
      listening = false;
      void stop.then((unlisten) => unlisten());
    };
  }, [open, layer]);
}
