// The touch and phone rules are keyed on the root's marks (`markTheLayout`),
// written out in each stylesheet that has them, and asked in code by
// `usePhoneLayout`. A copy that drifted, a mark spelt another way here or a
// pointer media query there, would split the page between two layouts on some
// phone, or give a phone with a pen the desktop's layout.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { PHONE_LYING_DOWN_LAYOUT, TOUCH_LAYOUT } from "./phoneLayout";

const SRC = fileURLToPath(new URL("..", import.meta.url));

function stylesheets(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return stylesheets(path);
    return entry.name.endsWith(".css") ? [path] : [];
  });
}

const sheets = stylesheets(SRC).map((path) => ({ path: path.slice(SRC.length), css: readFileSync(path, "utf8") }));

/** Every selector on a layout mark, with the element it is on. */
const marks = sheets.flatMap(({ path, css }) =>
  [...css.matchAll(/[\w-]*\[data-(?:touch|phone)\b[^\]]*\]/g)].map((match) => ({ path, mark: match[0] })),
);

describe("the stylesheets' touch and phone rules", () => {
  it("select the root's marks as shared/phoneLayout writes them", () => {
    expect(marks.filter(({ mark }) => mark === TOUCH_LAYOUT).length).toBeGreaterThan(0);
    expect(marks.filter(({ mark }) => mark === PHONE_LYING_DOWN_LAYOUT).length).toBeGreaterThan(0);
    expect(marks.filter(({ mark }) => mark !== TOUCH_LAYOUT && mark !== PHONE_LYING_DOWN_LAYOUT)).toEqual([]);
  });

  it("never ask the pointer, which Android reports as fine with a pen, a mouse or a touchpad", () => {
    const pointerQueries = sheets.flatMap(({ path, css }) =>
      [...css.matchAll(/@media\s+([^{]*)\{/g)]
        .filter((match) => /pointer/.test(match[1]))
        .map((match) => ({ path, condition: match[1].trim() })),
    );
    expect(pointerQueries).toEqual([]);
  });
});
