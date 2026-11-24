// Whether the client is in front, which decides how a notification is
// delivered. Tauri says "not focused" on Android whatever is in front, so
// there the page's visibility answers instead.

import { afterEach, describe, expect, it, vi } from "vitest";

const isFocused = vi.hoisted(() => vi.fn(() => Promise.resolve(false)));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isFocused }),
  UserAttentionType: { Informational: 2 },
}));

import { native } from "./native";

const ANDROID =
  "Mozilla/5.0 (Linux; Android 16; SM-S908B Build/BP2A.250605.031; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/142.0.0.0 Mobile Safari/537.36";
const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

afterEach(() => {
  vi.unstubAllGlobals();
  isFocused.mockClear();
});

describe("native.isWindowFocused", () => {
  it("on Android, is the page's visibility", async () => {
    vi.stubGlobal("navigator", { userAgent: ANDROID });
    vi.stubGlobal("document", { visibilityState: "visible" });
    await expect(native.isWindowFocused()).resolves.toBe(true);
    vi.stubGlobal("document", { visibilityState: "hidden" });
    await expect(native.isWindowFocused()).resolves.toBe(false);
    expect(isFocused).not.toHaveBeenCalled();
  });

  it("on the desktop, is the window's own focus", async () => {
    vi.stubGlobal("navigator", { userAgent: WINDOWS });
    vi.stubGlobal("document", { visibilityState: "visible" });
    await expect(native.isWindowFocused()).resolves.toBe(false);
    expect(isFocused).toHaveBeenCalledOnce();
  });
});
