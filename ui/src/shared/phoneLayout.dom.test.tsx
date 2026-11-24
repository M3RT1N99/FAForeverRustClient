// @vitest-environment happy-dom
//
// `usePhoneLayout` answers for the screen the page is on, and again when it
// changes: a phone lying down, not upright, not a tablet, and not a desktop
// window as short with a mouse.

import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { usePhoneLayout } from "./phoneLayout";
import "../testing/mounted";
import { DESKTOP, PHONE_LYING_DOWN, setScreen, SHORT_DESKTOP, type Screen } from "../testing/screens";

function Probe() {
  return <output>{usePhoneLayout() ? "phone" : "other"}</output>;
}

const answer = () => screen.getByRole("status").textContent;

describe("usePhoneLayout", () => {
  it.each([
    ["a phone lying down", PHONE_LYING_DOWN, "phone"],
    ["the test emulator lying down", { width: 866, height: 364, touch: true }, "phone"],
    ["a phone upright", { width: 412, height: 860, touch: true }, "other"],
    ["a tablet lying down", { width: 1280, height: 752, touch: true }, "other"],
    ["a desktop window as short, with a mouse", SHORT_DESKTOP, "other"],
    ["a desktop window", DESKTOP, "other"],
  ] as [string, Screen, string][])("says whether %s is laid out as a phone", (_, on, expected) => {
    setScreen(on);
    render(<Probe />);
    expect(answer()).toBe(expected);
  });

  it("follows the phone turned upright and back", async () => {
    setScreen(PHONE_LYING_DOWN);
    render(<Probe />);
    expect(answer()).toBe("phone");
    setScreen({ ...PHONE_LYING_DOWN, width: 340, height: 785 });
    await waitFor(() => expect(answer()).toBe("other"));
    setScreen(PHONE_LYING_DOWN);
    await waitFor(() => expect(answer()).toBe("phone"));
  });
});
