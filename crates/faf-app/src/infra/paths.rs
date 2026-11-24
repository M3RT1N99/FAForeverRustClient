//! The configured directory overrides, and the resolution every path helper
//! in this crate runs through.
//!
//! Process-global on purpose. The helpers that need this - `maps::maps_dir`,
//! `mods::mods_dir`, `faf_content::vault_dir`, `replay::local_replays_dir` and
//! their neighbours - are free functions called from a dozen places, several
//! of them synchronous and none of them holding a reference to settings.
//! Threading a configuration object through all of them would be a much larger
//! change than the feature warrants, and the value is written twice in a
//! session at most.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use faf_domain::state::{PathPreferences, ResolvedPaths};

use crate::ports::paths::PathsPort;

fn overrides() -> &'static RwLock<PathPreferences> {
    static OVERRIDES: OnceLock<RwLock<PathPreferences>> = OnceLock::new();
    OVERRIDES.get_or_init(|| RwLock::new(PathPreferences::default()))
}

/// Read one configured path.
///
/// A poisoned lock means a writer panicked while holding it, which would only
/// happen if cloning a `String` panicked. Falling back to "nothing configured"
/// keeps every lookup working instead of taking the client down with it.
fn configured(select: impl Fn(&PathPreferences) -> &String) -> Option<PathBuf> {
    let guard = overrides().read().ok()?;
    let value = select(&guard).trim();
    (!value.is_empty()).then(|| PathBuf::from(value))
}

pub(crate) fn vault_dir() -> Option<PathBuf> {
    configured(|paths| &paths.vault_dir)
}

pub(crate) fn maps_dir() -> Option<PathBuf> {
    configured(|paths| &paths.maps_dir)
}

pub(crate) fn mods_dir() -> Option<PathBuf> {
    configured(|paths| &paths.mods_dir)
}

pub(crate) fn replays_dir() -> Option<PathBuf> {
    configured(|paths| &paths.replays_dir)
}

pub(crate) fn game_prefs_path() -> Option<PathBuf> {
    configured(|paths| &paths.game_prefs_path)
}

pub(crate) fn map_generator_dir() -> Option<PathBuf> {
    configured(|paths| &paths.map_generator_dir)
}

/// The Wine prefix the game runs in, when one is configured.
///
/// Read by the launcher, which passes it to the game as `WINEPREFIX`, and by
/// [`crate::infra::mods`], which has to find the `game.prefs` *inside* it.
/// Both have to agree or enabling a mod writes to a file the game never reads.
pub(crate) fn wine_prefix() -> Option<PathBuf> {
    configured(|paths| &paths.wine_prefix)
}

pub(crate) fn java_path() -> Option<PathBuf> {
    configured(|paths| &paths.java_path)
}

/// The original game the player picked, which the game updater copies the
/// engine's libraries from. Not validated here: the updater checks it like
/// every other candidate, since a remembered install can have moved since.
pub(crate) fn original_game_dir() -> Option<PathBuf> {
    configured(|paths| &paths.original_game_dir)
}

/// The folders an Android app's client keeps its files in, as the variables
/// every default path is resolved from: `HOME`, which they are all below, is
/// the app's own files folder, and `TMPDIR`, where temporary files go, its
/// cache folder. Made from the app's data and cache folders as Tauri's path
/// API names them; a relative one is refused.
///
/// The shell sets them in the client's own environment before anything
/// resolves a path. The app's Kotlin used to, with `Os.setenv`, which writes
/// the environment of the system's C library. An arm64 client on an x86_64
/// device (an emulator, a Chromebook) runs translated, on a C library of its
/// own whose environment never had them, and there every default came out
/// relative, below the `/` an app starts in.
pub fn android_folders(
    data_dir: &Path,
    cache_dir: &Path,
) -> Result<[(&'static str, PathBuf); 2], String> {
    if let Some(folder) = [data_dir, cache_dir]
        .into_iter()
        .find(|folder| !folder.has_root())
    {
        return Err(format!(
            "the app's folder {} is not an absolute path",
            folder.display()
        ));
    }
    // Android's own files folder (`Context.getFilesDir`) is `files` in the
    // data folder. HOME has always been that one, so nothing a client kept
    // there moves.
    Ok([
        ("HOME", data_dir.join("files")),
        ("TMPDIR", cache_dir.to_path_buf()),
    ])
}

/// `path`, a default folder or file the client writes, checked on Android.
///
/// The defaults are below the home folder, and with none they fall back to a
/// path relative to the working directory: on the desktop the folder the
/// client was started from, as it always was. An Android app starts in `/`,
/// which nothing can be written to, so there a relative default is a replay
/// list that stays empty and a settings save that fails with "Read-only file
/// system", far from the cause. Android's HOME is the app's own folder
/// ([`android_folders`]); a relative default there means it was not set, and
/// the client stops on it here, naming the path, instead of writing under `/`.
pub(crate) fn checked_default(android: bool, path: PathBuf) -> PathBuf {
    // `has_root`: on Android the same as `is_absolute`, and an Android path
    // reads as one in the tests on Windows too.
    assert!(
        !android || path.has_root(),
        "the default {} is relative: Android's HOME was not set when it was resolved",
        path.display()
    );
    path
}

/// The adapter behind [`PathsPort`]. Stateless: the state is the global above,
/// which is what every path helper reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfiguredPaths;

impl PathsPort for ConfiguredPaths {
    fn set_overrides(&self, preferences: PathPreferences) {
        if let Ok(mut guard) = overrides().write() {
            *guard = preferences;
        }
    }

    /// Ask each helper what it would use right now, which is the only honest
    /// answer: the fallback chains live in those helpers and differ per
    /// location.
    fn resolved(&self) -> ResolvedPaths {
        let display = |path: PathBuf| path.display().to_string();
        ResolvedPaths {
            vault_dir: display(crate::infra::faf_content::vault_dir()),
            maps_dir: display(crate::infra::maps::maps_dir()),
            mods_dir: display(crate::infra::mods::mods_dir()),
            replays_dir: display(crate::infra::replay::local_replays_dir()),
            game_prefs_path: display(crate::infra::mods::game_prefs_path()),
            map_generator_dir: display(crate::infra::map_generator::generator_dir()),
            java_path: crate::infra::java_runtime::preferred_java_path(),
            // Empty on Windows rather than a path nothing would use: the row
            // is not shown there, and reporting a `.wine` directory as the
            // prefix "a launch would use" would be a lie.
            wine_prefix: crate::infra::mods::resolved_wine_prefix()
                .map(display)
                .unwrap_or_default(),
            // Empty when there is none: the banner's cue, and the launch's.
            original_game_dir: crate::infra::game_updater::find_retail_install()
                .map(display)
                .unwrap_or_default(),
        }
    }
}

/// Inert path port for tests and the offline shell.
///
/// Deliberately records nothing into the global: the free functions then fall
/// through to the environment and discovery, which is what every test that
/// sets a `FAF_*` variable expects.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakePaths;

impl PathsPort for FakePaths {
    fn set_overrides(&self, _preferences: PathPreferences) {}

    fn resolved(&self) -> ResolvedPaths {
        ResolvedPaths::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overrides are one process-wide value, and `cargo test` runs these
    /// on threads of the same process. Without this the two tests race: one
    /// resets the overrides while the other is between setting them and
    /// reading them back, and the failure lands on whichever lost, in a run
    /// that has nothing to do with either of them.
    static OVERRIDES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Exclusive use of the overrides, restored to empty on the way out.
    ///
    /// Poisoning is ignored on purpose: a panicking test has already failed
    /// and there is nothing here to corrupt, so taking the lock anyway keeps
    /// one failure from being reported as two.
    fn with_overrides(preferences: PathPreferences, check: impl FnOnce()) {
        let _guard = OVERRIDES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ConfiguredPaths.set_overrides(preferences);
        check();
        ConfiguredPaths.set_overrides(PathPreferences::default());
    }

    #[test]
    fn an_unset_field_reads_as_no_override() {
        with_overrides(PathPreferences::default(), || {
            assert_eq!(maps_dir(), None);
        });
    }

    #[test]
    fn a_configured_field_wins_and_is_trimmed() {
        // A path pasted out of a file manager routinely arrives with a
        // trailing space; resolving that literally would find nothing.
        with_overrides(
            PathPreferences {
                maps_dir: "  C:/faf/maps  ".into(),
                ..PathPreferences::default()
            },
            || {
                assert_eq!(maps_dir(), Some(PathBuf::from("C:/faf/maps")));
                assert_eq!(mods_dir(), None);
            },
        );
    }

    /// The settings service remembers the original game when it is picked;
    /// this is how the updater, which has no settings of its own, hears of it.
    #[test]
    fn the_picked_original_game_reaches_the_updater() {
        with_overrides(
            PathPreferences {
                original_game_dir:
                    "D:/SteamLibrary/steamapps/common/Supreme Commander Forged Alliance".into(),
                ..PathPreferences::default()
            },
            || {
                assert_eq!(
                    original_game_dir(),
                    Some(PathBuf::from(
                        "D:/SteamLibrary/steamapps/common/Supreme Commander Forged Alliance"
                    ))
                );
            },
        );
    }

    const ANDROID_APP: &str = "/data/user/0/com.faforever.rustclient";

    #[test]
    fn android_folders_are_the_apps_files_and_cache_folders() {
        let app = Path::new(ANDROID_APP);
        assert_eq!(
            android_folders(app, &app.join("cache")).unwrap(),
            [("HOME", app.join("files")), ("TMPDIR", app.join("cache"))]
        );
        assert!(android_folders(Path::new("files"), &app.join("cache")).is_err());
        assert!(android_folders(app, Path::new("")).is_err());
    }

    /// The Android path choice, from the folders Tauri names to where the
    /// client's files go: each default it writes is below the app's own
    /// folder. Without a HOME (the v6 test build's arm64 client, translated
    /// on an x86_64 emulator) each falls back to a relative path, below the
    /// `/` an app starts in: the desktop keeps that fallback, Android stops
    /// on it.
    #[test]
    fn no_android_default_is_relative() {
        use crate::infra::{faf_content, game, mods, replay, settings_file};

        let app = Path::new(ANDROID_APP);
        let [(_, home), (_, temp)] = android_folders(app, &app.join("cache")).unwrap();
        // The folders `directories` names below HOME on Linux, which it takes
        // Android for, with no XDG variable set.
        let config = home.join(".config").join("faforeverclient");
        let local_data = home.join(".local").join("share");
        let faf_root = game::linux_faf_root_from(&home, None, |_| false);

        let mut defaults = vec![
            settings_file::settings_path_in(Some(&config)),
            mods::game_prefs_in(&local_data),
            temp,
        ];
        defaults.extend(faf_content::default_vault_dirs_in(false, None, Some(&home)));
        defaults.extend(replay::default_replays_dirs(
            false,
            home.to_str().unwrap(),
            Some(faf_root),
            None,
        ));
        for path in defaults {
            assert!(path.starts_with(app), "{} is not the app's", path.display());
            assert_eq!(checked_default(true, path.clone()), path);
        }

        let without_home = [
            settings_file::settings_path_in(None),
            mods::game_prefs_in(Path::new(".")),
            faf_content::default_vault_dirs_in(false, None, None).remove(0),
            replay::default_replays_dirs(false, "", None, None).remove(0),
        ];
        for path in without_home {
            assert!(!path.has_root(), "{}", path.display());
            assert_eq!(checked_default(false, path.clone()), path, "the desktop's");
            assert!(
                std::panic::catch_unwind(|| checked_default(true, path.clone())).is_err(),
                "Android took {}",
                path.display()
            );
        }
    }

    /// The map generator is built while the client starts, before Settings is
    /// read. It used to take its folders at that moment, so a player whose
    /// maps live somewhere else had every generated map written to the default
    /// folder, which the game never mounts, and the map was never found.
    #[test]
    fn the_map_generator_follows_folders_configured_after_it_was_built() {
        let config = crate::infra::MapGeneratorConfig::faf();
        with_overrides(
            PathPreferences {
                maps_dir: "C:/faf/maps".into(),
                map_generator_dir: "C:/faf/generator".into(),
                ..PathPreferences::default()
            },
            || {
                assert_eq!(config.maps_dir(), PathBuf::from("C:/faf/maps"));
                assert_eq!(config.generator_dir(), PathBuf::from("C:/faf/generator"));
            },
        );
    }
}
