// @vitest-environment happy-dom
//
// The page's viewport on Android, at the system's font sizes 1.0, 1.15 and
// 1.3: laid out at the webview's own size and drawn at 100 % whatever the font
// size, which grows the text alone through the webview's text zoom, as Chrome
// grows a page's text. The interface scale zooms the whole page. The client
// log hears of each new layout once, with the font size and the text zoom
// MainActivity handed over. The zoom it makes of a font size is its own, and
// covered on the JVM (TextZoomTest); here the page only passes it on.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyAndroidLayout, resetAndroidLayoutForTests } from "./androidViewport";

/** The client log's line, as the page hands it to the shell. */
const report = vi.hoisted(() => vi.fn());
vi.mock("../ipc/native", () => ({ native: { reportAndroidLayout: report } }));

/** The S22 Ultra's webview in HD+ lying down: 1544 x 720 at 300 dpi, less the bars and the cutout. */
const WEBVIEW = { width: 749, height: 354 };

const handedOver = window as unknown as Record<string, unknown>;
let viewport: HTMLMetaElement;

const drawnAt = (width: number, scale: string) =>
  `width=${width}, initial-scale=${scale}, minimum-scale=${scale}, maximum-scale=${scale}`;

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

describe("the page's viewport on Android", () => {
  it.each([
    [1, 100],
    [1.15, 115],
    [1.3, 130],
  ])("is the webview's own size at 100 % at the font size %s, and the log gets the font size and text zoom handed over", (font, zoom) => {
    handedOver.fafAndroidFontScale = font;
    handedOver.fafAndroidTextZoom = zoom;
    applyAndroidLayout(100);
    expect(viewport.content).toBe(drawnAt(WEBVIEW.width, "1.0000"));
    expect(report).toHaveBeenCalledOnce();
    expect(report).toHaveBeenCalledWith(
      expect.objectContaining({
        webviewWidth: WEBVIEW.width,
        webviewHeight: WEBVIEW.height,
        devicePixelRatio: window.devicePixelRatio,
        fontScale: font,
        textZoom: zoom,
        interfaceScale: 100,
        layoutWidth: WEBVIEW.width,
        layoutScale: 1,
        phone: true,
      }),
    );
  });

  it("zooms the whole page with the interface scale alone, whatever font size was handed over", () => {
    handedOver.fafAndroidFontScale = 1.3;
    handedOver.fafAndroidTextZoom = 130;
    applyAndroidLayout(125);
    expect(viewport.content).toBe(drawnAt(599, (WEBVIEW.width / 599).toFixed(4)));
    expect(report).toHaveBeenCalledWith(expect.objectContaining({ interfaceScale: 125, layoutWidth: 599 }));
  });

  it("tells the log of a layout once, not again for the same one", () => {
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
