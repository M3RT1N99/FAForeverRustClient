// The shell's answer about play, asked once before the first render.

import { afterEach, describe, expect, it, vi } from "vitest";

const playOffered = vi.hoisted(() => vi.fn<() => Promise<boolean>>());
vi.mock("../ipc/native", () => ({ native: { playOffered } }));

import { playIsOffered, setPlayOffered } from "./platform";
import { loadPlayOffer } from "./playOffer";

const ANDROID = "Mozilla/5.0 (Linux; Android 16; SM-S908B; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/142.0.0.0 Mobile Safari/537.36";

function inTheShell() {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.stubGlobal("navigator", { userAgent: ANDROID });
}

afterEach(() => {
  vi.unstubAllGlobals();
  playOffered.mockReset();
  setPlayOffered(null);
});

describe("loadPlayOffer", () => {
  it("takes the shell's answer: an Android build with the flag plays", async () => {
    inTheShell();
    playOffered.mockResolvedValue(true);
    await loadPlayOffer();
    expect(playIsOffered()).toBe(true);
  });

  it("leaves the platform's answer to a shell without the command", async () => {
    inTheShell();
    playOffered.mockRejectedValue(new Error("unknown command play_offered"));
    await loadPlayOffer();
    expect(playIsOffered()).toBe(false);
  });

  it("asks nothing outside the shell", async () => {
    await loadPlayOffer();
    expect(playOffered).not.toHaveBeenCalled();
    expect(playIsOffered()).toBe(true);
  });
});
