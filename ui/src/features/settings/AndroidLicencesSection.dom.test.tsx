// @vitest-environment happy-dom

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { AndroidLicencesSection } from "./AndroidLicencesSection";

const { clientNotices } = vi.hoisted(() => ({ clientNotices: vi.fn() }));
vi.mock("../../ipc/client");
vi.mock("../../ipc/native", () => ({ native: { clientNotices } }));

failOnConsoleError();
afterEach(() => vi.resetAllMocks());

const showButton = () => screen.getByRole("button", { name: en["android.licences.show"] });

describe("the open-source licences on Android", () => {
  it("shows the notice the build carries, and closes it", async () => {
    clientNotices.mockResolvedValue("FAForever client\n\nMIT License\n...");
    const user = userEvent.setup();
    render(<AndroidLicencesSection />);
    await user.click(showButton());
    const text = await screen.findByRole("textbox", { name: en["android.licences.title"] });
    expect((text as HTMLTextAreaElement).value).toContain("MIT License");
    // The text button, not the dialog's own close cross, which is named the same.
    await user.click(screen.getByText(en["settings.diagnostics.close"], { selector: "button" }));
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("says so for a build without the notice", async () => {
    clientNotices.mockResolvedValue(null);
    const user = userEvent.setup();
    render(<AndroidLicencesSection />);
    await user.click(showButton());
    const text = await screen.findByRole("textbox", { name: en["android.licences.title"] });
    expect((text as HTMLTextAreaElement).value).toBe(en["android.licences.missing"]);
  });

  it("says why the notice could not be read", async () => {
    clientNotices.mockRejectedValue(new Error("could not read the licences: no plugin"));
    const user = userEvent.setup();
    render(<AndroidLicencesSection />);
    await user.click(showButton());
    expect(await screen.findByText("could not read the licences: no plugin")).toBeTruthy();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});
