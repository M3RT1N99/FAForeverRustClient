// The phone's media query is written out in each stylesheet that has rules
// for a phone lying down, and asked in code by `usePhoneLayout`. A copy that
// drifted, a 480 here or a missing `pointer: coarse` there, would split the
// page between two layouts at some size, or give a desktop with a mouse the
// phone's rules.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { PHONE_LYING_DOWN } from "./phoneLayout";

const SRC = fileURLToPath(new URL("..", import.meta.url));

function stylesheets(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return stylesheets(path);
    return entry.name.endsWith(".css") ? [path] : [];
  });
}

/** Every media condition in the client's stylesheets that asks about the pointer. */
const pointerConditions = stylesheets(SRC).flatMap((path) =>
  [...readFileSync(path, "utf8").matchAll(/@media\s+([^{]*pointer[^{]*)\{/g)].map((match) => ({
    path: path.slice(SRC.length),
    condition: match[1].trim(),
  })),
);

describe("the phone's media query", () => {
  it("is written the same in every stylesheet with rules for a phone lying down", () => {
    const lyingDown = pointerConditions.filter(({ condition }) => condition !== "(pointer: coarse)");
    expect(lyingDown.length).toBeGreaterThan(0);
    expect(lyingDown.filter(({ condition }) => condition !== PHONE_LYING_DOWN)).toEqual([]);
  });

  it("asks for a touch screen, so a desktop with a mouse never gets a phone's rules", () => {
    expect(PHONE_LYING_DOWN).toContain("(pointer: coarse)");
    // Below the desktop window's smallest height at 100 % (window.rs).
    expect(PHONE_LYING_DOWN).toContain("(max-height: 479px)");
  });
});
