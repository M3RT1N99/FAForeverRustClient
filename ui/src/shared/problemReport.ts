// A problem report from the client: GitHub's new-issue page, with the build
// and the device already in it.
//
// Nothing about the player goes into the address: no login, no path, no
// address of theirs. The logs travel apart, as the zip "Export logs" makes
// and the player attaches to the issue, because a link cannot carry a file.
// The body is English, like every issue on the tracker.

import type { ReportContext } from "../ipc/native";

/**
 * Where reports go: the fork that publishes the Android test builds, until
 * Android is part of the client project's own releases, then
 * `FAForeverRustClient/FAForeverRustClient`.
 */
export const REPORT_REPOSITORY = "M3RT1N99/FAForeverRustClient";

export function problemReportBody(context: ReportContext): string {
  return [
    "**What happened?**",
    "",
    "",
    "**What did you expect?**",
    "",
    "",
    "**Steps to reproduce**",
    "1. ",
    "",
    "**Logs**",
    "Attach the zip from Settings, Diagnostics, Export logs.",
    "",
    "---",
    `Client: ${context.version} (${context.commit})`,
    `System: ${context.system}`,
    `Device: ${context.device}`,
  ].join("\n");
}

/**
 * GitHub's new-issue page for a problem with this build on this device. The
 * title says it is Android's: a label in the address would be dropped for
 * anybody without triage rights, the players who report.
 */
export function problemReportUrl(context: ReportContext): string {
  const query = new URLSearchParams({
    title: "Android: ",
    body: problemReportBody(context),
  });
  return `https://github.com/${REPORT_REPOSITORY}/issues/new?${query.toString()}`;
}
