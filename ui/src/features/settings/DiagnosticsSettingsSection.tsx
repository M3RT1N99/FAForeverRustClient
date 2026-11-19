import { useState } from "react";
import { Button } from "../../design-system/Button";
import { Modal } from "../../design-system/Modal";
import { StatusNotice } from "../../design-system/StatusNotice";
import { native, type LogIssue, type LogKind, type LogPreview } from "../../ipc/native";
import { openExternalUrl } from "../../shared/externalLinks";
import { plainError } from "../../shared/plainError";

/**
 * Where an issue's "More information" goes, empty when there is nothing useful
 * to link to. The URL is data about the issue, but it lives here rather than in
 * the domain because `externalLinks` owns the host allowlist that vets it: this
 * thread is on `forum.faforever.com`, which that allowlist already covers.
 *
 * Same target as the Java client's `helpLinks.soundIssues`.
 */
const LOG_ISSUE_HELP: Record<LogIssue, string> = {
  gameMinimized: "",
  soundDriver:
    "https://forum.faforever.com/topic/4084/solutions-for-snd-error-xact-invalid-arg-xact3dapply-failed",
};
import { SettingRow, SettingsGroup } from "./SettingControls";
import { useTranslation } from "../../i18n/useTranslation";
import { foldersCanBeOpened, theGameIsItsOwnApp } from "../../shared/platform";
import { openHttpsUrl } from "../../shared/externalLinks";
import { problemReportUrl } from "../../shared/problemReport";

/**
 * What the last press left to say: that there are no logs yet, or that the
 * shell refused, with its reason and the press to repeat.
 */
type Outcome = { kind: "noLogs" } | { kind: "failed"; reason: string; retry: () => void };

export function DiagnosticsSettingsSection() {
  const { t } = useTranslation();
  const [preview, setPreview] = useState<LogPreview | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const openFolder = (kind: LogKind) => {
    setOutcome(null);
    void native.openLogFolder(kind).catch((reason) =>
      setOutcome({ kind: "failed", reason: String(reason), retry: () => openFolder(kind) }));
  };
  const viewLatest = (kind: LogKind) => {
    setOutcome(null);
    void native.readLatestLog(kind)
      .then((result) => result ? setPreview(result) : setOutcome({ kind: "noLogs" }))
      .catch((reason) =>
        setOutcome({ kind: "failed", reason: String(reason), retry: () => viewLatest(kind) }));
  };

  const exportLogs = () => {
    setOutcome(null);
    void native.shareClientLogs(t("settings.diagnostics.exportTitle")).catch((reason) =>
      setOutcome({ kind: "failed", reason: String(reason), retry: exportLogs }));
  };
  const reportProblem = () => {
    setOutcome(null);
    void native.reportContext()
      .then((context) => openHttpsUrl(problemReportUrl(context)))
      .catch((reason) => setOutcome({ kind: "failed", reason: String(reason), retry: reportProblem }));
  };

  // On Android the game's logs are the game app's, and no file manager can
  // open this app's folders: the client's own log is read here, and leaves
  // the device through the share sheet, scrubbed.
  const openable = foldersCanBeOpened();

  return (
    <SettingsGroup>
      {!theGameIsItsOwnApp() && (
        <SettingRow label={t("settings.diagnostics.gameLogs")} hint={t("settings.diagnostics.gameLogsHint")}>
          <div className="settings-diagnostic-actions">
            <Button onClick={() => viewLatest("game")}>{t("settings.diagnostics.viewLatest")}</Button>
            {openable && <Button onClick={() => openFolder("game")}>{t("settings.diagnostics.openFolder")}</Button>}
          </div>
        </SettingRow>
      )}
      <SettingRow label={t("settings.diagnostics.clientLogs")} hint={t("settings.diagnostics.clientLogsHint")}>
        <div className="settings-diagnostic-actions">
          <Button onClick={() => viewLatest("client")}>{t("settings.diagnostics.viewLatest")}</Button>
          {openable && <Button onClick={() => openFolder("client")}>{t("settings.diagnostics.openFolder")}</Button>}
          {!openable && <Button onClick={exportLogs}>{t("settings.diagnostics.exportLogs")}</Button>}
        </div>
      </SettingRow>
      {!openable && (
        <SettingRow label={t("settings.diagnostics.report")} hint={t("settings.diagnostics.reportHint")}>
          <div className="settings-diagnostic-actions">
            <Button onClick={reportProblem}>{t("settings.diagnostics.reportOpen")}</Button>
          </div>
        </SettingRow>
      )}
      {outcome?.kind === "noLogs" && (
        <p className="settings-inline-error" role="alert">{t("settings.diagnostics.noLogs")}</p>
      )}
      {/* The shell's reason in a plain sentence, its own words ("could not
          read latest log: ...") on hover, and the same press again. */}
      {outcome?.kind === "failed" && (
        <StatusNotice
          tone="error"
          className="settings-inline-notice"
          action={{ label: t("common.retry"), onClick: outcome.retry }}
          detail={outcome.reason}
        >
          {plainError(outcome.reason)}
        </StatusNotice>
      )}
      {preview && (
        <Modal className="diagnostic-log-modal" onClose={() => setPreview(null)}>
          <h2>{preview.fileName}</h2>
          {/* Above the log, not buried under it: the whole point is that these
              traces are invisible in thousands of lines unless you already know
              the string to search for. */}
          {preview.issues.length > 0 && (
            <section className="log-analysis" role="status">
              <p className="log-analysis-heading">{t("log.analysis.heading")}</p>
              <ul>
                {preview.issues.map((issue) => (
                  <li key={issue}>
                    <span>{t(`log.analysis.${issue}`)}</span>
                    {LOG_ISSUE_HELP[issue] && (
                      <button
                        type="button"
                        className="log-analysis-link"
                        onClick={() => void openExternalUrl(LOG_ISSUE_HELP[issue])}
                      >
                        {t("log.analysis.moreInfo")}
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </section>
          )}
          <p className="muted">{t("settings.diagnostics.truncated")}</p>
          <textarea
            readOnly
            value={preview.content}
            aria-label={t("settings.diagnostics.contentsOf", { file: preview.fileName })}
          />
          <div className="settings-diagnostic-actions">
            <Button onClick={() => void navigator.clipboard.writeText(preview.content)}>{t("settings.diagnostics.copy")}</Button>
            <Button variant="primary" onClick={() => setPreview(null)}>{t("settings.diagnostics.close")}</Button>
          </div>
        </Modal>
      )}
    </SettingsGroup>
  );
}
