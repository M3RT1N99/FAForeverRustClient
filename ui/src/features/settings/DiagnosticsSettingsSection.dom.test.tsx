// @vitest-environment happy-dom

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { en } from "../../i18n/catalog/en";
import { failOnConsoleError } from "../../testing/consoleGuard";
import { DiagnosticsSettingsSection } from "./DiagnosticsSettingsSection";

const { shareClientLogs } = vi.hoisted(() => ({ shareClientLogs: vi.fn() }));
vi.mock("../../ipc/client");
vi.mock("../../ipc/native", () => ({ native: { shareClientLogs } }));
// The Android client: no folder to open, so the logs leave through the share sheet.
vi.mock("../../shared/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/platform")>()),
  foldersCanBeOpened: () => false,
  theGameIsItsOwnApp: () => true,
}));

failOnConsoleError();
afterEach(() => vi.resetAllMocks());

const exportButton = () => screen.getByRole("button", { name: en["settings.diagnostics.exportLogs"] });

describe("exporting the logs on Android", () => {
  it("says it is collecting them until the share sheet is up", async () => {
    let shareSheetUp!: (missing: string | null) => void;
    shareClientLogs.mockReturnValue(new Promise((resolve) => { shareSheetUp = resolve; }));
    const user = userEvent.setup();
    render(<DiagnosticsSettingsSection />);
    await user.click(exportButton());
    const busy = screen.getByRole("button", { name: en["settings.diagnostics.exporting"] });
    expect((busy as HTMLButtonElement).disabled).toBe(true);
    shareSheetUp(null);
    expect(await screen.findByRole("button", { name: en["settings.diagnostics.exportLogs"] })).toBeTruthy();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("says why the game app's logs are not in the export", async () => {
    shareClientLogs.mockResolvedValue("could not write the zip: no space left on the device");
    const user = userEvent.setup();
    render(<DiagnosticsSettingsSection />);
    await user.click(exportButton());
    expect(await screen.findByText(/no space left on the device/)).toBeTruthy();
    expect(exportButton()).toBeTruthy();
  });

  it("says why the export failed, and repeats it", async () => {
    shareClientLogs.mockRejectedValueOnce(new Error("could not share the logs: no plugin")).mockResolvedValueOnce(null);
    const user = userEvent.setup();
    render(<DiagnosticsSettingsSection />);
    await user.click(exportButton());
    await user.click(await screen.findByRole("button", { name: en["common.retry"] }));
    expect(shareClientLogs).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(exportButton()).toBeTruthy();
  });
});
