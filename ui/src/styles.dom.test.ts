// @vitest-environment happy-dom
//
// The page's floor: the desktop window's smallest width, which a phone lying
// down at a large font size or interface scale is narrower than.

import { expect, it } from "vitest";
import { addStylesheets, PHONE_LYING_DOWN, SHORT_DESKTOP, setScreen } from "./testing/screens";

it("lets a phone lying down lay the page out narrower than the desktop's smallest window", () => {
  setScreen({ ...PHONE_LYING_DOWN, width: 520 });
  addStylesheets("ui/src/styles.css");
  expect(getComputedStyle(document.body).minWidth).toBe("0");
});

it("keeps the floor on a desktop window of the same size", () => {
  setScreen({ ...SHORT_DESKTOP, width: 520 });
  addStylesheets("ui/src/styles.css");
  expect(getComputedStyle(document.body).minWidth).toBe("560px");
});
