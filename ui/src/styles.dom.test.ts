// @vitest-environment happy-dom
//
// The page's floor: the desktop window's smallest width, which an Android
// window can be narrower than, at a large font size or interface scale or in
// a small window, whatever pointer the phone reports.

import { expect, it } from "vitest";
import {
  addStylesheets,
  PHONE_LYING_DOWN,
  PHONE_WITH_A_PEN,
  SHORT_DESKTOP,
  SHORT_TOUCH_DESKTOP,
  setScreen,
  type Screen,
} from "./testing/screens";

it.each([
  ["a phone lying down", { ...PHONE_LYING_DOWN, width: 520 }],
  ["a phone with its pen, which reports a fine pointer", { ...PHONE_WITH_A_PEN, width: 520 }],
  ["an upright window on Android", { ...PHONE_LYING_DOWN, width: 412, height: 560 }],
] as [string, Screen][])("lets %s lay the page out narrower than the desktop's smallest window", (_, on) => {
  setScreen(on);
  addStylesheets("ui/src/styles.css");
  expect(getComputedStyle(document.body).minWidth).toBe("0");
});

it.each([
  ["with a mouse", SHORT_DESKTOP],
  ["on a touch screen", SHORT_TOUCH_DESKTOP],
] as [string, Screen][])("keeps the floor on a desktop window of the same size %s", (_, on) => {
  setScreen({ ...on, width: 520 });
  addStylesheets("ui/src/styles.css");
  expect(getComputedStyle(document.body).minWidth).toBe("560px");
});
