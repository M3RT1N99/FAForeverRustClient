// Settings, Android: the licences of everything this app is built from, as
// its build writes them into the APK (scripts/android-notices). Apache-2.0
// asks for them where such notices normally appear, and this is where a
// player looks for them; the text is shown as the log preview shows a log.

import { useState } from "react";
import { Button } from "../../design-system/Button";
import { Modal } from "../../design-system/Modal";
import { StatusNotice } from "../../design-system/StatusNotice";
import { useTranslation } from "../../i18n/useTranslation";
import { native } from "../../ipc/native";
import { SettingRow, SettingsGroup } from "./SettingControls";

export function AndroidLicencesSection() {
  const { t } = useTranslation();
  const [text, setText] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const show = () => {
    setFailure(null);
    native.clientNotices().then(
      (notices) => setText(notices ?? t("android.licences.missing")),
      (error: unknown) => setFailure(error instanceof Error ? error.message : String(error)),
    );
  };

  return (
    <SettingsGroup>
      <SettingRow label={t("android.licences.title")} hint={t("android.licences.hint")}>
        <Button onClick={show}>{t("android.licences.show")}</Button>
      </SettingRow>
      {failure && (
        <StatusNotice tone="error" className="settings-inline-notice">
          {failure}
        </StatusNotice>
      )}
      {text !== null && (
        <Modal className="diagnostic-log-modal" onClose={() => setText(null)}>
          <h2>{t("android.licences.title")}</h2>
          <textarea readOnly value={text} aria-label={t("android.licences.title")} />
          <div className="settings-diagnostic-actions">
            <Button variant="primary" onClick={() => setText(null)}>
              {t("settings.diagnostics.close")}
            </Button>
          </div>
        </Modal>
      )}
    </SettingsGroup>
  );
}
