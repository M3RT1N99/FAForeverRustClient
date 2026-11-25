// @vitest-environment happy-dom
//
// The page's viewport on Android, and the line the client log gets for it:
// each new layout once, with the screen, the webview, the font size and the
// text zoom MainActivity handed over, and the layout chosen.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyAndroidLayout, resetAndroidLayoutForTests } from "./androidViewport";

/** The client log's line, as the page hands it to the shell. */
const report = vi.hoisted(() => vi.fn());
vi.mock("../ipc/native", () => ({ native: { reportAndroidLayout: report } }));

/** The S22 Ultra's webview in HD+ lying down: 1544 x 720 at 300 dpi, less the bars and the cutout. */
const WEBVIEW = { width: 749, height: 354 };

const handedOver = window as unknown as Record<string, unknown>;
let viewport: HTMLMetaElement;

beforeEach(() => {
  viewport = document.createElement("meta");
  viewport.name = "viewport";
  viewport.content = "width=device-width, initial-scale=1.0";
  document.head.appendChild(viewport);
  // The visual viewport of a page laid out at the webview's width at 100 %.
  Object.defineProperty(window, "visualViewport", {
    configurable: true,
    value: { width: WEBVIEW.width, height: WEBVIEW.height, scale: 1 },
  });
  resetAndroidLayoutForTests();
  report.mockClear();
});

afterEach(() => {
  viewport.remove();
  Reflect.deleteProperty(window, "visualViewport");
  delete handedOver.fafAndroidFontScale;
  delete handedOver.fafAndroidTextZoom;
});

describe("the line the client log gets for the page's layout on Android", () => {
  it("names the webview, the font size, the text zoom, the interface scale and the layout chosen", () => {
    handedOver.fafAndroidFontScale = 1.15;
    handedOver.fafAndroidTextZoom = 100;
    applyAndroidLayout(100);
    const width = Math.round(WEBVIEW.width / 1.15);
    expect(viewport.content).toContain(`width=${width},`);
    expect(report).toHaveBeenCalledOnce();
    expect(report).toHaveBeenCalledWith(
      expect.objectContaining({
        webviewWidth: WEBVIEW.width,
        webviewHeight: WEBVIEW.height,
        devicePixelRatio: window.devicePixelRatio,
        fontScale: 1.15,
        textZoom: 100,
        interfaceScale: 100,
        layoutWidth: width,
        layoutScale: WEBVIEW.width / width,
        phone: true,
      }),
    );
  });

  it("is written once for a layout, not again for the same one", () => {
    applyAndroidLayout(100);
    applyAndroidLayout(100);
    expect(report).toHaveBeenCalledOnce();
    applyAndroidLayout(125);
    expect(report).toHaveBeenCalledTimes(2);
  });

  it("says the font size is not known where MainActivity could not hand it over", () => {
    applyAndroidLayout(100);
    expect(report).toHaveBeenCalledWith(
      expect.objectContaining({ fontScale: null, textZoom: null, layoutWidth: WEBVIEW.width }),
    );
  });
});
