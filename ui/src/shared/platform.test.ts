// What the interface decides from the user agent alone. Android's says Linux
// too, which is how its build came to offer Wine, a folder picker the shell
// refuses, and adapters it does not have.

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  foldersCanBePicked,
  gameNeedsALaunchWrapper,
  runsOnAndroid,
  startsProgramsBesideTheGame,
  steamStatusIsSupported,
  theGameIsItsOwnApp,
} from "./platform";

const AGENTS = {
  windows: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0",
  linux: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
  android: "Mozilla/5.0 (Linux; Android 16; SM-S908B Build/BP2A.250605.031; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/142.0.0.0 Mobile Safari/537.36",
} as const;

function on(agent: keyof typeof AGENTS) {
  vi.stubGlobal("navigator", { userAgent: AGENTS[agent] });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("platform", () => {
  it("offers the desktop's game, folders and programs on Linux", () => {
    on("linux");
    expect(runsOnAndroid()).toBe(false);
    expect(gameNeedsALaunchWrapper()).toBe(true);
    expect(theGameIsItsOwnApp()).toBe(false);
    expect(foldersCanBePicked()).toBe(true);
    expect(startsProgramsBesideTheGame()).toBe(true);
    expect(steamStatusIsSupported()).toBe(true);
  });

  it("needs no wrapper on Windows", () => {
    on("windows");
    expect(gameNeedsALaunchWrapper()).toBe(false);
    expect(foldersCanBePicked()).toBe(true);
    expect(startsProgramsBesideTheGame()).toBe(true);
  });

  it("offers none of it on Android, whose user agent also says Linux", () => {
    on("android");
    expect(runsOnAndroid()).toBe(true);
    expect(theGameIsItsOwnApp()).toBe(true);
    expect(gameNeedsALaunchWrapper()).toBe(false);
    expect(foldersCanBePicked()).toBe(false);
    expect(startsProgramsBesideTheGame()).toBe(false);
    expect(steamStatusIsSupported()).toBe(false);
  });
});
