//! Fixtures the replay submodules' tests share: a stub map generator, replay
//! preparation wired to it, a game app, mod files and a version for the game
//! app's path, and replay bodies built byte by byte.
//!
//! One place rather than a copy per submodule, because the bodies are only
//! trustworthy as long as every test builds them the same way.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use faf_domain::protocol::vault_query::ModVaultQuery;
use faf_domain::state::{
    GeneratorOptionQuery, GeneratorOptions, GeneratorPreset, GeneratorStatus, InstalledMod,
    ModDownloadSize, ModDownloadTarget, VaultMod,
};
use tokio::sync::mpsc;

use crate::ports::game_app::{GameAppPort, GameAppRun, GameAppStart};
use crate::ports::{
    GamePreparation, GameUpdaterPort, GeneratorUpdate, ModPrepFailure, ModSearchPage, ModsPort,
    UpdateProgress,
};

use super::playback::GameAppContent;
use super::preparation::ReplayPreparation;
use super::ReplayConfig;

/// A game app that takes every start, or refuses each with `refusal`.
#[derive(Default)]
pub(super) struct RecordingGameApp {
    pub(super) starts: std::sync::Mutex<Vec<GameAppStart>>,
    pub(super) refusal: Option<String>,
}

#[async_trait]
impl GameAppPort for RecordingGameApp {
    async fn start(&self, start: GameAppStart) -> Result<GameAppRun, String> {
        self.starts.lock().unwrap().push(start);
        if let Some(refusal) = &self.refusal {
            return Err(refusal.clone());
        }
        let (_, ended) = tokio::sync::oneshot::channel();
        Ok(GameAppRun { ended })
    }
}

/// FAF's vault as the game app's path asks it: the file of a mod version by
/// its uid, for the ones listed, and nothing else. The rest of the port is
/// never reached from a replay.
struct ModFiles(HashMap<String, String>);

#[async_trait]
impl ModsPort for ModFiles {
    async fn mod_file(&self, uid: &str) -> Result<String, String> {
        self.0
            .get(uid)
            .cloned()
            .ok_or_else(|| format!("the vault knows no mod version {uid}"))
    }

    async fn list_vault(&self) -> Result<Vec<VaultMod>, String> {
        unimplemented!("a replay never lists the vault")
    }
    async fn search_vault(&self, _query: ModVaultQuery) -> Result<ModSearchPage, String> {
        unimplemented!()
    }
    async fn list_installed(&self) -> Result<Vec<InstalledMod>, String> {
        unimplemented!()
    }
    async fn download_sizes(&self, _targets: Vec<ModDownloadTarget>) -> Vec<ModDownloadSize> {
        unimplemented!()
    }
    async fn install_mod(
        &self,
        _uid: String,
        _download_url: String,
    ) -> Result<Vec<InstalledMod>, String> {
        unimplemented!()
    }
    async fn update_mod(
        &self,
        _uid: String,
        _folder_name: String,
        _download_url: String,
    ) -> Result<Vec<InstalledMod>, String> {
        unimplemented!()
    }
    async fn uninstall_mod(&self, _folder_name: String) -> Result<Vec<InstalledMod>, String> {
        unimplemented!()
    }
    async fn toggle_mod(&self, _uid: String, _enabled: bool) -> Result<Vec<InstalledMod>, String> {
        unimplemented!()
    }
    async fn set_active_mods(&self, _uids: Vec<String>) -> Result<Vec<InstalledMod>, String> {
        unimplemented!()
    }
    async fn ensure_game_mods(
        &self,
        _mods: &BTreeMap<String, String>,
        _replace_conflicts: bool,
    ) -> Result<(), ModPrepFailure> {
        unimplemented!()
    }
}

/// The updater of a client whose game keeps its own files: asked which
/// version is current, and never to prepare an install.
struct CurrentVersion(Result<Option<i32>, String>);

#[async_trait]
impl GameUpdaterPort for CurrentVersion {
    async fn prepare(&self, _request: GamePreparation) -> mpsc::Receiver<UpdateProgress> {
        unimplemented!("nothing is prepared for a game that keeps its own files")
    }

    async fn current_version(&self, featured_mod: &str) -> Result<Option<i32>, String> {
        assert_eq!(featured_mod, "faf");
        self.0.clone()
    }
}

/// What the game app is told to fetch, from a vault that knows `files`
/// (`uid`, file) and an updater that answers `version` for `faf`.
pub(super) fn game_app_content(
    files: &[(&str, &str)],
    version: Result<Option<i32>, String>,
) -> GameAppContent {
    GameAppContent {
        updater: Arc::new(CurrentVersion(version)),
        mods: Arc::new(ModFiles(
            files
                .iter()
                .map(|(uid, file)| ((*uid).to_owned(), (*file).to_owned()))
                .collect(),
        )),
    }
}

/// A body's mod table as the engine writes it: one table of `mod_info.lua`
/// fields per mod (`uid`, `name`, `ui_only`), keyed by its place in the list.
pub(super) fn lua_mod_table(mods: &[(&str, &str, bool)]) -> Vec<u8> {
    let mut table = vec![4];
    for (index, (uid, name, ui_only)) in mods.iter().enumerate() {
        table.extend(lua_number(index as f32 + 1.0));
        table.push(4);
        table.extend(lua_string("uid"));
        table.extend(lua_string(uid));
        table.extend(lua_string("name"));
        table.extend(lua_string(name));
        table.extend(lua_string("ui_only"));
        table.extend_from_slice(&[3, u8::from(*ui_only)]);
        table.push(5);
    }
    table.push(5);
    table
}

/// The opening of a `.scfareplay` stream, up to and including `mod_table`:
/// how a decompressed body begins, and what the replay server sends first.
pub(super) fn stream_opening(mod_table: &[u8]) -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(b"Supreme Commander v1.50.3817\0\r\n\0");
    stream.extend_from_slice(b"Replay v1.9\r\n/maps/SCMP_009/SCMP_009.scmap\0\r\n\x1a\0");
    stream.extend_from_slice(&(mod_table.len() as u32).to_le_bytes());
    stream.extend_from_slice(mod_table);
    stream
}

/// Records what the launch path asked of the generator. Everything the
/// replay path never touches is left unimplemented rather than faked, so a
/// future call through one of those reads as a test bug, not as a pass.
pub(super) struct StubGenerator {
    installed: bool,
    outcome: GeneratorStatus,
    pub(super) asked_for: std::sync::Mutex<Vec<String>>,
}

impl StubGenerator {
    pub(super) fn new(installed: bool, outcome: GeneratorStatus) -> Self {
        Self {
            installed,
            outcome,
            asked_for: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl crate::ports::MapGeneratorPort for StubGenerator {
    fn is_installed(&self, _map_name: &str) -> bool {
        self.installed
    }

    async fn generate_named(&self, map_name: String) -> mpsc::Receiver<GeneratorUpdate> {
        self.asked_for.lock().unwrap().push(map_name);
        let (tx, rx) = mpsc::channel(4);
        let _ = tx.send(GeneratorUpdate::Status(self.outcome.clone())).await;
        rx
    }

    async fn generate(&self, _options: GeneratorOptions) -> mpsc::Receiver<GeneratorUpdate> {
        unimplemented!("the replay path never generates from options")
    }
    async fn query_options(
        &self,
        _query: GeneratorOptionQuery,
        _version: Option<String>,
        _progress: Option<mpsc::Sender<GeneratorUpdate>>,
    ) -> Result<Vec<String>, String> {
        unimplemented!()
    }
    async fn preflight(&self, _options: GeneratorOptions) -> Result<String, String> {
        unimplemented!()
    }
    async fn help(&self, _version: Option<String>) -> Result<String, String> {
        unimplemented!()
    }
    fn cancel(&self) {
        unimplemented!()
    }
    async fn save_preset(&self, _name: &str, _options: &GeneratorOptions) -> Result<(), String> {
        unimplemented!()
    }
    async fn list_presets(&self) -> Vec<GeneratorPreset> {
        unimplemented!()
    }
    async fn delete_preset(&self, _name: &str) -> Result<(), String> {
        unimplemented!()
    }
    async fn rename_preset(&self, _name: &str, _new_name: &str) -> Result<(), String> {
        unimplemented!()
    }
    async fn latest_version(&self) -> Result<String, String> {
        unimplemented!()
    }
    async fn available_versions(&self) -> Result<Vec<String>, String> {
        unimplemented!()
    }
    async fn clean_up(&self, _protected: &[String]) -> Result<usize, String> {
        unimplemented!()
    }
    async fn map_previews(
        &self,
        _map_names: &[String],
    ) -> std::collections::HashMap<String, String> {
        unimplemented!()
    }
}

/// Replay preparation on its own, with the stub as its map generator.
pub(super) fn preparation_with(generator: Arc<StubGenerator>) -> ReplayPreparation {
    ReplayPreparation::new(&replay_config(), generator)
}

pub(super) fn replay_config() -> ReplayConfig {
    ReplayConfig {
        user_api_base: "https://user.faforever.com".into(),
        api_base: "https://api.faforever.com".into(),
        vault_host: "https://replay.faforever.com".into(),
        replay_target_dir: None,
        exe_name: "ForgedAlliance.exe".into(),
        content_base: "https://content.faforever.com".into(),
    }
}

pub(super) fn lua_string(value: &str) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
    bytes
}

pub(super) fn lua_number(value: f32) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// One army of the replay body's table, the way the engine writes it:
/// the player's name, their seat's team, and the TrueSkill pair the
/// displayed rating is derived from.
fn lua_army_named(name: &str, team: f32, civilian: bool) -> Vec<u8> {
    let mut bytes = vec![4];
    bytes.extend(lua_string("PlayerName"));
    bytes.extend(lua_string(name));
    bytes.extend(lua_string("Team"));
    bytes.extend(lua_number(team));
    bytes.extend(lua_string("Civilian"));
    bytes.push(3);
    bytes.push(u8::from(civilian));
    bytes.extend(lua_string("Faction"));
    bytes.extend(lua_number(1.0));
    bytes.extend(lua_string("MEAN"));
    bytes.extend(lua_number(1500.0));
    bytes.extend(lua_string("DEV"));
    bytes.extend(lua_number(100.0));
    bytes.extend(lua_string("Country"));
    bytes.extend(lua_string("DE"));
    bytes.push(5);
    bytes
}

fn local_body_with_army() -> Vec<u8> {
    // Four bytes, as every real replay has here and as the reference
    // parser reads. This fixture used to carry a bare `\0`, which only
    // parsed because the reader looked for a NUL instead of a width.
    local_body_with_army_after(b"\r\n\x1a\0")
}

/// [`local_body_with_army`] with the four-byte field after the map string
/// filled by the caller, so a test can put bytes there that are not a
/// NUL-terminated string.
pub(super) fn local_body_with_army_after(gap: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"Supreme Commander v1.50.3764\0");
    body.extend_from_slice(b"\r\n\0");
    body.extend_from_slice(b"Replay v1.9\r\n/maps/SCMP_009/SCMP_009.scmap\0");
    body.extend_from_slice(gap);
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&[4, 5]);
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&[4, 5]);
    body.push(1);
    body.extend_from_slice(b"TestPlayer\0");
    body.extend_from_slice(&u32::MAX.to_le_bytes());
    body.push(0);
    // Three armies: the two players the envelope also knows about, on the
    // two teams the engine seated them in, and the neutral civilian army
    // every map carries and no listing should show.
    body.push(3);
    for army in [
        lua_army_named("TestPlayer", 2.0, false),
        lua_army_named("Guest", 3.0, false),
        lua_army_named("civilian", 1.0, true),
    ] {
        body.extend_from_slice(&1_u32.to_le_bytes());
        body.extend(army);
        body.extend_from_slice(&[0, 0]);
    }
    body
}

/// [`local_body_with_army`], framed the way a `.fafreplay` carries it:
/// zlib behind its uncompressed size, base64 over the pair.
pub(super) fn qcompressed_body() -> String {
    use base64::Engine as _;
    use std::io::Write as _;

    let body = local_body_with_army();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&body).unwrap();
    let mut qcompressed = (body.len() as u32).to_be_bytes().to_vec();
    qcompressed.extend(encoder.finish().unwrap());
    base64::engine::general_purpose::STANDARD.encode(qcompressed)
}
