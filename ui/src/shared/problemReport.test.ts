import { describe, expect, it } from "vitest";
import { problemReportUrl, REPORT_REPOSITORY } from "./problemReport";

const CONTEXT = {
  version: "1.0.0",
  commit: "4b5169f0",
  system: "Android 16 (API 36)",
  device: "samsung SM-S908B",
};

describe("problemReportUrl", () => {
  it("opens the tracker's new-issue form with the build and the device in the body", () => {
    const url = new URL(problemReportUrl(CONTEXT));
    expect(`${url.origin}${url.pathname}`).toBe(`https://github.com/${REPORT_REPOSITORY}/issues/new`);
    expect(url.searchParams.get("title")).toBe("Android: ");
    const body = url.searchParams.get("body") ?? "";
    expect(body).toContain("Client: 1.0.0 (4b5169f0)");
    expect(body).toContain("System: Android 16 (API 36)");
    expect(body).toContain("Device: samsung SM-S908B");
    expect(body).toContain("Attach the zip");
  });

  it("carries nothing but the build, the system and the device", () => {
    // The address is all a report has before the player writes in it: the
    // template, and the four facts about the build.
    const url = new URL(problemReportUrl(CONTEXT));
    expect([...url.searchParams.keys()].sort()).toEqual(["body", "title"]);
    const body = url.searchParams.get("body") ?? "";
    const facts = body.split("\n").filter((line) => /^(Client|System|Device):/.test(line));
    expect(facts).toHaveLength(3);
    expect(body).not.toMatch(/\/data\/|login|@|\d+\.\d+\.\d+\.\d+/);
  });
});
