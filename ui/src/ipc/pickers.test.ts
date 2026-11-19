// Every file and folder picker, and Android.
//
// Android's dialog picks no folder, and the file it hands back is a content
// address the client cannot read, so the shell refuses both there. A test
// build still showed that refusal to a player: a picker nobody had gated. Two
// things keep that from coming back:
//
// - `native.selectFile` (and `selectDirectory`, through it) is the one way to
//   the shell's `pick_path`, and on Android it answers in a plain sentence
//   without asking the shell;
// - every place that opens a picker is listed here with the Android gate that
//   keeps its control off the screen there. A new picker fails this test until
//   it has a gate and is listed.

import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path }));

import { native } from "./native";
import { en } from "../i18n/catalog/en";

const SRC = fileURLToPath(new URL("..", import.meta.url));

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sources(path);
    if (!/\.tsx?$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name)) return [];
    return [path];
  });
}

const FILES = sources(SRC).map((path) => ({
  name: relative(SRC, path).split(sep).join("/"),
  text: readFileSync(path, "utf8"),
}));

/** The shared helpers that open a picker: their callers are pickers too. */
const HELPERS = ["selectFile", "selectDirectory", "pickOriginalGameFolder", "openUploadFromDisk"];

/**
 * Every file that opens a picker, and what keeps its control off Android: the
 * predicate from `shared/platform`, and the file it is checked in when that is
 * a parent. `helper` marks a file that only defines one of the helpers above.
 */
const PICKERS: Record<string, { gate: string; in?: string } | { helper: true }> = {
  "ipc/native.ts": { helper: true },
  "shared/originalGame.ts": { helper: true },
  "features/uploads/UploadDialog.tsx": { helper: true },
  "features/maps/GeneratorAdvancedOptions.tsx": { gate: "foldersCanBePicked" },
  "features/maps/MapsView.tsx": { gate: "foldersCanBePicked" },
  "features/mods/ModsView.tsx": { gate: "foldersCanBePicked" },
  "features/replays/local/LocalReplayView.tsx": {
    gate: "filesCanBePicked",
    in: "features/replays/local/LocalReplaySearch.tsx",
  },
  "features/settings/BackgroundSettings.tsx": { gate: "filesCanBePicked" },
  "features/settings/NotificationsSettingsSection.tsx": { gate: "filesCanBePicked" },
  "features/settings/CompanionProgramsSettings.tsx": {
    gate: "startsProgramsBesideTheGame",
    in: "features/settings/GameSettingsSection.tsx",
  },
  "features/settings/GamePathsSection.tsx": {
    gate: "theGameIsItsOwnApp",
    in: "features/settings/PathsSettingsSection.tsx",
  },
  "features/settings/PathsSettingsSection.tsx": { gate: "theGameIsItsOwnApp" },
  "features/shell/InstallBanner.tsx": { gate: "theGameIsItsOwnApp" },
};

/** Whether `text` calls one of the helpers: `name(`, but not `function name(`. */
function opensAPicker(text: string): boolean {
  return HELPERS.some((name) => new RegExp(`(?<!function |\\w)${name}\\s*\\(`).test(text));
}

describe("pickers", () => {
  it("reach the shell's pick_path through native.ts alone", () => {
    expect(FILES.filter((file) => file.text.includes("\"pick_path\"")).map((file) => file.name)).toEqual([
      "ipc/native.ts",
    ]);
    expect(FILES.filter((file) => file.text.includes("@tauri-apps/plugin-dialog")).map((file) => file.name)).toEqual(
      [],
    );
  });

  it("are each listed with the Android gate that hides them", () => {
    const found = FILES.filter((file) => opensAPicker(file.text)).map((file) => file.name).sort();
    expect(found, "a picker was added or removed: give it an Android gate and list it in PICKERS").toEqual(
      Object.keys(PICKERS).sort(),
    );
    for (const [name, entry] of Object.entries(PICKERS)) {
      if ("helper" in entry) continue;
      const where = entry.in ?? name;
      const gateFile = FILES.find((file) => file.name === where);
      expect(gateFile, `${where} is gone`).toBeDefined();
      expect(gateFile?.text, `${name}: ${entry.gate}() is not checked in ${where}`).toMatch(
        new RegExp(`\\b${entry.gate}\\(\\)`),
      );
    }
  });
});

describe("native.selectFile", () => {
  const ANDROID =
    "Mozilla/5.0 (Linux; Android 16; SM-S908B Build/BP2A.250605.031; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/142.0.0.0 Mobile Safari/537.36";
  const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

  beforeEach(() => invoke.mockReset());
  afterEach(() => vi.unstubAllGlobals());

  it("refuses on Android in a plain sentence, without asking the shell", async () => {
    vi.stubGlobal("navigator", { userAgent: ANDROID });
    await expect(native.selectFile({ title: "x" })).rejects.toThrow(en["errors.cause.notOnAndroid"]);
    await expect(native.selectDirectory()).rejects.toThrow(en["errors.cause.notOnAndroid"]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("asks the shell everywhere else", async () => {
    vi.stubGlobal("navigator", { userAgent: LINUX });
    invoke.mockResolvedValue("/home/player/a.fafreplay");
    await expect(native.selectDirectory("/home/player")).resolves.toBe("/home/player/a.fafreplay");
    expect(invoke).toHaveBeenCalledWith("pick_path", { request: { directory: true, defaultPath: "/home/player" } });
  });
});
