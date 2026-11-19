// Settings, Android: the FAF game app, the game this client starts on
// Android. Its version and updates, what of its game data it has, its own
// setup for what is missing, and an offline game.

import { useState } from "react";
import { Button } from "../../design-system/Button";
import { StatusNotice } from "../../design-system/StatusNotice";
import { useTranslation } from "../../i18n/useTranslation";
import type { GameAppData, GameAppStatus } from "../../ipc/native";
import {
  GAME_APP_NEED_TEXT,
  gameAppNeed,
  gameAppNeedAction,
  playOfflineInGameApp,
  setUpInGameApp,
  useGameAppStatus,
} from "../../shared/gameApp";
import {
  installGameAppUpdate,
  lookForGameAppUpdate,
  useGameAppUpdateStore,
} from "../../shared/gameAppUpdate";
import { SettingRow, SettingsGroup } from "./SettingControls";

function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** FAF's newest version, when the game app knows of one it does not have. */
function newerVersion(data: GameAppData): number | null {
  const newest = data.newestKnown;
  return newest !== null && !data.gameVersions.includes(newest) ? newest : null;
}

/** What the hint of the updates' row says. */
function updateHint(
  t: ReturnType<typeof useTranslation>["t"],
  status: GameAppStatus,
  { offer, phase, looked }: ReturnType<typeof useGameAppUpdateStore.getState>,
): string {
  if (phase === "looking") return t("android.gameApp.update.looking");
  if (!offer) return t(looked ? "android.gameApp.update.upToDate" : "android.gameApp.update.notLooked");
  const version = { version: offer.version };
  if (phase === "installing") return t("android.gameApp.update.installing", version);
  if (status.gameData?.gameRunning) return t("android.gameApp.update.availableAfterGame", version);
  if (offer.metered) return t("android.gameApp.update.availableMetered", version);
  return t(status.silentUpdate ? "android.gameApp.update.available" : "android.gameApp.update.availableAsks", version);
}

/** The game app's updates: what the last look found, and the one press for it. */
function GameAppUpdateRow({ status }: { status: GameAppStatus }) {
  const { t } = useTranslation();
  const update = useGameAppUpdateStore();
  const { offer, phase, failure } = update;
  return (
    <>
      <SettingRow label={t("android.gameApp.update.title")} hint={updateHint(t, status, update)}>
        {offer ? (
          <Button
            disabled={phase !== "idle" || (status.gameData?.gameRunning ?? false)}
            onClick={() => void installGameAppUpdate()}
          >
            {t("android.gameApp.update.install")}
          </Button>
        ) : (
          <Button disabled={phase !== "idle"} onClick={() => void lookForGameAppUpdate()}>
            {t("android.gameApp.update.look")}
          </Button>
        )}
      </SettingRow>
      {failure && (
        <StatusNotice tone="error" className="settings-inline-notice">
          {failure}
        </StatusNotice>
      )}
    </>
  );
}

export function AndroidSettingsSection() {
  const { t } = useTranslation();
  const { status, refresh } = useGameAppStatus();
  const offer = useGameAppUpdateStore((state) => state.offer);
  const [failure, setFailure] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);

  // Not Android, or no answer yet.
  if (!status) return null;
  const need = gameAppNeed(status);
  const data = status.gameData;
  // A game app too old for the client takes the update below where one was
  // found, its releases' page otherwise.
  const appAction =
    need === "install" || need === "allow" || (need === "update" && !offer) ? gameAppNeedAction(need) : null;

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
          need === "install" || need === "update" || need === "allow"
            ? t(GAME_APP_NEED_TEXT[need].title)
            : t("android.gameApp.appVersion", { version: status.versionName })
        }
      >
        {appAction && <Button onClick={() => run(appAction.run)}>{t(appAction.label)}</Button>}
      </SettingRow>
      {status.installed && <GameAppUpdateRow status={status} />}
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
