// Settings, Android: the FAF game app, the game this client starts on
// Android. Its version, what of its game data it has, its own setup for what
// is missing, and an offline game.

import { useState } from "react";
import { Button } from "../../design-system/Button";
import { StatusNotice } from "../../design-system/StatusNotice";
import { useTranslation } from "../../i18n/useTranslation";
import type { GameAppData } from "../../ipc/native";
import {
  GAME_APP_NEED_TEXT,
  gameAppNeed,
  gameAppNeedAction,
  playOfflineInGameApp,
  setUpInGameApp,
  useGameAppStatus,
} from "../../shared/gameApp";
import { SettingRow, SettingsGroup } from "./SettingControls";

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** FAF's newest version, when the game app knows of one it does not have. */
function newerVersion(data: GameAppData): number | null {
  const newest = data.newestKnown;
  return newest !== null && !data.gameVersions.includes(newest) ? newest : null;
}

export function AndroidSettingsSection() {
  const { t } = useTranslation();
  const { status, refresh } = useGameAppStatus();
  const [failure, setFailure] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);

  // Not Android, or no answer yet.
  if (!status) return null;
  const need = gameAppNeed(status);
  const data = status.gameData;
  const appAction = need === "install" || need === "update" || need === "allow" ? gameAppNeedAction(need) : null;

  // Whatever came of it, the status is asked again once the player is back.
  const run = (task: () => Promise<void>) => {
    setFailure(null);
    void task()
      .catch((error: unknown) => setFailure(reasonOf(error)))
      .finally(refresh);
  };
  const play = () => {
    setPlaying(true);
    run(() => playOfflineInGameApp().finally(() => setPlaying(false)));
  };

  return (
    <SettingsGroup>
      <SettingRow
        label={t("android.gameApp.app")}
        hint={
          appAction
            ? t(GAME_APP_NEED_TEXT[need as "install" | "update" | "allow"].title)
            : t("android.gameApp.appVersion", { version: status.versionName })
        }
      >
        {appAction && <Button onClick={() => run(appAction.run)}>{t(appAction.label)}</Button>}
      </SettingRow>
      {data && (
        <>
          <SettingRow
            label={t("android.gameApp.scfa")}
            hint={
              data.scfaArchives > 0 && data.scfaFonts > 0
                ? t("android.gameApp.scfaFound", {
                    archives: data.scfaArchives,
                    fonts: data.scfaFonts,
                    sounds: data.scfaSoundBanks,
                    movies: data.scfaMovies,
                  })
                : t("android.gameApp.scfaMissing")
            }
          >
            <Button disabled={need === "busy"} onClick={() => run(() => setUpInGameApp("scfa"))}>
              {t("android.gameApp.scfaSetUp")}
            </Button>
          </SettingRow>
          <SettingRow
            label={t("android.gameApp.gameFiles")}
            hint={
              data.gameVersions.length === 0
                ? t("android.gameApp.gameFilesMissing")
                : newerVersion(data) !== null
                  ? t("android.gameApp.gameFilesNewer", {
                      versions: data.gameVersions.join(", "),
                      newest: newerVersion(data) ?? "",
                    })
                  : t("android.gameApp.gameFilesReady", { versions: data.gameVersions.join(", ") })
            }
          >
            <Button disabled={need === "busy"} onClick={() => run(() => setUpInGameApp("game_files"))}>
              {t("android.gameApp.gameFilesGet")}
            </Button>
          </SettingRow>
          {data.jobStep && (
            <StatusNotice tone="busy" className="settings-inline-notice">
              {t("android.gameApp.busy", { step: data.jobStep })}
            </StatusNotice>
          )}
          <SettingRow label={t("android.gameApp.play")} hint={t("android.gameApp.playHint")}>
            <Button disabled={need !== "ready" || data.gameRunning || playing} onClick={play}>
              {t("auth.playOffline")}
            </Button>
          </SettingRow>
        </>
      )}
      {failure && (
        <StatusNotice tone="error" className="settings-inline-notice">
          {failure}
        </StatusNotice>
      )}
    </SettingsGroup>
  );
}
