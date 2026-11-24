// Missing-install banner: slides in at the top of the workspace when the
// client cannot find Forged Alliance.
//
// Both reference clients refuse to get this far without a known install (the
// Python client's `validate_game_path`, the Java client's first-run wizard).
// This client is usable without one: chat, the vault, the leaderboard and the
// map/mod browsers are all pure API features: so instead of blocking startup
// it says what is missing and offers the one-click fix.
//
// The state is derived, not configured: the backend stats the paths on startup
// and after every change (see `faf-domain`'s install slice), so the banner also
// catches an install that was moved or uninstalled since it was configured.
//
// Two different things can be missing. FAF's own copy of the game, which the
// client downloads itself, and the original Forged Alliance the player owns,
// which FAF reads movies, sounds, fonts and libraries from and which nothing
// can download. The second used to surface only as a broken first game.

import { useState } from "react";
import { ipc } from "../../ipc/client";
import { native } from "../../ipc/native";
import { Button } from "../../design-system/Button";
import { Icon } from "../../design-system/Icon";
import { useAppStore } from "../../store/store";
import { t } from "../../i18n";
import { useTranslation } from "../../i18n/useTranslation";
import { pickOriginalGameFolder, setOriginalGameDir } from "../../shared/originalGame";
import { theGameIsItsOwnApp } from "../../shared/platform";
import { GAME_APP_NEED_TEXT, gameAppNeed, gameAppNeedAction, useGameAppStatus } from "../../shared/gameApp";
import { installGameAppUpdate, useGameAppUpdateStore } from "../../shared/gameAppUpdate";

const openSettings = () =>
  ipc.send({ kind: "Nav", command: { type: "select", payload: { tab: "settings" } } });

const checkAgain = () =>
  ipc.send({ kind: "Settings", command: { type: "checkInstalls" } });

/**
 * Set both installs at once: the common case is one FA folder for both.
 *
 * Not a component, so it uses the standalone `t` rather than the hook: the
 * dialog title is produced once, at click time, and needs the language that is
 * current then.
 */
async function pickInstall() {
  const path = await native.selectFile({
    title: t("shell.install.pickTitle"),
    filters: [{ name: "ForgedAlliance.exe", extensions: ["exe"] }],
  });
  if (!path) return;
  await ipc.settle({ kind: "Settings", command: { type: "setGamePath", payload: { path } } });
  await ipc.dispatch({
    kind: "Settings",
    command: { type: "setReplayGamePath", payload: { path } },
  });
}

export function InstallBanner() {
  const { t } = useTranslation();
  const install = useAppStore((s) => s.state.install);
  const [dismissed, setDismissed] = useState(false);

  // `checked` guards the startup window: before the first stat completes we
  // know nothing, and flashing a warning at every launch would train the user
  // to ignore it.
  if (!install.checked || dismissed) return null;

  // On Android the game is FAF's game app, with its own files: there is no
  // install here to locate and no folder to choose, so the banner says where
  // games run instead of offering either.
  if (theGameIsItsOwnApp()) return <GameIsItsOwnApp onDismiss={() => setDismissed(true)} />;

  // FAF's own install that is neither there nor about to be downloaded comes
  // first: without it nothing starts, whatever else is found.
  const installMissing = !install.gameReady && !install.gamePending;
  if (!installMissing && !install.resolved.originalGameDir) {
    return <OriginalGameMissing onDismiss={() => setDismissed(true)} />;
  }
  if (install.gameReady) return null;

  // A machine that has never run FAF is not missing an install; it has not
  // downloaded one yet, and pressing Play is what downloads it. Saying "not
  // found, go and locate it" there is what sent the reporter of #261 to the
  // Java client and back: the only executable he had to point at was Steam's,
  // which this client cannot use, and the folder it was asking for did not
  // exist until something created it.
  const pending = install.gamePending;

  // Replays can play from a separate install, so a half-configured client gets
  // a narrower message than a completely unconfigured one.
  const replayOnly = install.replayReady;

  return (
    <div className="install-banner" role="status">
      <span className="install-banner-icon" aria-hidden="true">
        <Icon name={pending ? "download" : "lock"} size={16} />
      </span>
      <div className="install-banner-copy">
        <strong>
          {pending
            ? t("shell.install.notDownloadedYet")
            : replayOnly
              ? t("shell.install.noGameConfigured")
              : t("shell.install.notFound")}
        </strong>
        <span className="muted">
          {pending
            ? t("shell.install.notDownloadedYetHint")
            : replayOnly
              ? t("shell.install.replayOnlyHint")
              : t("shell.install.missingHint")}
        </span>
      </div>
      <div className="install-banner-actions">
        {/* Not the primary action any more when the client is about to do
            this itself: the button is there for somebody who keeps their
            install somewhere else, not for somebody who has to find one. */}
        <Button variant={pending ? "ghost" : "primary"} onClick={() => ipc.run(pickInstall())}>
          {t("shell.install.locate")}
        </Button>
        <Button onClick={() => void openSettings()}>{t("shell.install.settings")}</Button>
        <Button onClick={() => void checkAgain()} title={t("shell.install.checkAgainTitle")}>
          {t("shell.install.checkAgain")}
        </Button>
        <button
          type="button"
          className="install-banner-close"
          aria-label={t("shell.install.dismiss")}
          title={t("shell.install.dismissTitle")}
          onClick={() => setDismissed(true)}
        >
          <Icon name="close" size={14} />
        </button>
      </div>
    </div>
  );
}

/**
 * FAF's install is there or on its way, but the game the player owns is not.
 *
 * Its own banner rather than a variant of the one above: the fix is a
 * different folder (the game from Steam, GOG or a disc, never FAF's), and the
 * button opens the same folder picker as the Forged Alliance row in Settings,
 * Paths.
 * Launches on Windows are refused until it is found, so this is the last
 * place to learn it before a game would have started.
 */
function OriginalGameMissing({ onDismiss }: { onDismiss: () => void }) {
  const { t } = useTranslation();
  const choose = () =>
    ipc.run(
      pickOriginalGameFolder().then((path) => (path ? setOriginalGameDir(path) : undefined)),
    );

  return (
    <div className="install-banner" role="status">
      <span className="install-banner-icon" aria-hidden="true">
        <Icon name="alert" size={16} />
      </span>
      <div className="install-banner-copy">
        <strong>{t("shell.install.originalMissing")}</strong>
        <span className="muted">{t("shell.install.originalMissingHint")}</span>
      </div>
      <div className="install-banner-actions">
        <Button variant="primary" onClick={choose}>
          {t("shell.install.chooseOriginal")}
        </Button>
        <Button onClick={() => void checkAgain()} title={t("shell.install.checkAgainTitle")}>
          {t("shell.install.checkAgain")}
        </Button>
        <button
          type="button"
          className="install-banner-close"
          aria-label={t("shell.install.dismiss")}
          title={t("shell.install.dismissTitle")}
          onClick={onDismiss}
        >
          <Icon name="close" size={14} />
        </button>
      </div>
    </div>
  );
}

/**
 * Android: the game is FAF's game app, not a file or a folder on this device,
 * so nothing here can locate or choose it. The banner says what the game app
 * still needs, with the one press that answers it (its update, its releases,
 * or its own setup), and is gone once it is ready.
 */
function GameIsItsOwnApp({ onDismiss }: { onDismiss: () => void }) {
  const { t } = useTranslation();
  const { status, refresh } = useGameAppStatus();
  const update = useGameAppUpdateStore();
  if (!status) return null;
  const need = gameAppNeed(status);
  if (need === "ready") return null;
  const text = GAME_APP_NEED_TEXT[need];
  const updateOffered = need === "update" && update.offer !== null;
  const action = updateOffered
    ? { label: "android.gameApp.update.install" as const, run: installGameAppUpdate }
    : gameAppNeedAction(need);

  return (
    <div className="install-banner" role="status">
      <span className="install-banner-icon" aria-hidden="true">
        <Icon name={need === "busy" ? "refresh" : "info"} size={16} />
      </span>
      <div className="install-banner-copy">
        <strong>{t(text.title)}</strong>
        {text.hint && <span className="muted">{t(text.hint)}</span>}
        {need === "busy" && status.gameData?.jobStep && (
          <span className="muted">{t("android.gameApp.busy", { step: status.gameData.jobStep })}</span>
        )}
        {updateOffered && update.phase === "installing" && update.offer && (
          <span className="muted">{t("android.gameApp.update.installing", { version: update.offer.version })}</span>
        )}
        {updateOffered && update.failure && <span className="muted">{update.failure}</span>}
      </div>
      <div className="install-banner-actions">
        {action && (
          <Button
            variant="primary"
            disabled={update.phase === "installing"}
            onClick={() => ipc.run(action.run().finally(refresh))}
          >
            {t(action.label)}
          </Button>
        )}
        <button
          type="button"
          className="install-banner-close"
          aria-label={t("shell.install.dismiss")}
          title={t("shell.install.dismissTitle")}
          onClick={onDismiss}
        >
          <Icon name="close" size={14} />
        </button>
      </div>
    </div>
  );
}
