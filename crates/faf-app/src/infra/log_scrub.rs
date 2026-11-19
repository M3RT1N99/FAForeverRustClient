//! A log line as it may leave the device.
//!
//! The client's logs go into a bug report, or to whoever helps (the export in
//! `log_export`). What a line can carry that is nobody else's business is
//! taken out first:
//!
//! - credentials: `Authorization` headers, bearer tokens, cookies, OAuth
//!   tokens and a redirect's `code` and `state`, JWTs;
//! - the faf-uid's machine proof (`unique_id`), and any long encoded blob;
//! - addresses on the player's own network, and the hosts of a test server
//!   the client was pointed at;
//! - e-mail addresses, and the home folder's path;
//! - other players' names, as `player1`, `player2`... in the order they first
//!   appear, the same name always the same number. The player's own login
//!   stays: a report is about them.
//!
//! The name rules follow what this client's own log statements carry: a
//! `login=` field, the `channel=` of a private conversation (a nick, where a
//! public channel starts with `#`), an IRC prefix. Each rule is pinned by a
//! test on a line as the client writes it.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::{Captures, Regex};

fn rule(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a scrub rule is a valid pattern")
}

static AUTHORIZATION: LazyLock<Regex> = LazyLock::new(|| {
    rule(
        r#"(?i)\b((?:proxy-)?authorization)(["']?\s*[:=]\s*["']?)(?:(?:bearer|basic)\s+)?[^\s"',}]+"#,
    )
});
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| rule(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}"));
static SECRET_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    rule(
        r#"(?i)\b(access_token|refresh_token|id_token|client_secret|code_verifier|password|passwd|session_token|api_key|apikey|unique_id)(["']?\s*[:=]\s*["']?)[^\s"'&,;}]+"#,
    )
});
static QUERY_SECRET: LazyLock<Regex> =
    LazyLock::new(|| rule(r#"(?i)([?&](?:code|state|token|key|sig|signature|auth)=)[^&\s"'#]+"#));
static COOKIE: LazyLock<Regex> =
    LazyLock::new(|| rule(r#"(?i)\b((?:set-)?cookie)(["']?\s*[:=]\s*["']?)[^"\r\n]+"#));
static JWT: LazyLock<Regex> =
    LazyLock::new(|| rule(r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}"));
/// Longer than any hash this client logs (a SHA-256 is 64 hex digits): the
/// faf-uid's output and other encoded payloads.
static BLOB: LazyLock<Regex> = LazyLock::new(|| rule(r"[A-Za-z0-9+/_-]{80,}={0,2}"));
static IRC_PREFIX: LazyLock<Regex> = LazyLock::new(|| rule(r":([^\s!:@]+)![^\s@]+@\S+"));
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    rule(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b")
});
static PRIVATE_V4: LazyLock<Regex> = LazyLock::new(|| {
    rule(
        r"\b(?:10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])|192\.168|100\.(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7])|169\.254)\.\d{1,3}\.\d{1,3}\b",
    )
});
static PRIVATE_V6: LazyLock<Regex> =
    LazyLock::new(|| rule(r"(?i)\b(?:f[cd][0-9a-f]{2}|fe[89ab][0-9a-f]):[0-9a-f:]*[0-9a-f]"));
static PRIVATE_HOST: LazyLock<Regex> =
    LazyLock::new(|| rule(r"(?i)\b(?:[a-z0-9-]+\.)+(?:ts\.net|local|lan|home\.arpa|internal)\b"));
static NAME_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    rule(
        r#"\b(login|player|nick|nickname|sender|peer|user|username|opponent|recipient)=("(?:[^"\\]|\\.)*"|[^\s,]+)"#,
    )
});
static NAME_LIST: LazyLock<Regex> =
    LazyLock::new(|| rule(r"\b(players|nicks|logins)=\[([^\]]*)\]"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| rule(r#""((?:[^"\\]|\\.)*)""#));
/// A private conversation's channel is the other player's nick; a public
/// channel starts with `#` and stays.
static PRIVATE_CHANNEL: LazyLock<Regex> =
    LazyLock::new(|| rule(r#"\bchannel=("?)([^#\s"][^\s",]*)"#));

/// What one export knows about whose log this is. One scrubber per export, so
/// a name is the same `playerN` in every file of it.
pub struct Scrubber {
    own_login: Option<String>,
    home: Option<String>,
    servers: Vec<String>,
    names: HashMap<String, String>,
}

impl Scrubber {
    /// `own_login` is the player's own name, which stays. `home` is the home
    /// folder's path, written as `~`. `servers` are the hosts of a server the
    /// client was pointed at instead of FAF's, written as `[server]`.
    pub fn new(
        own_login: Option<&str>,
        home: Option<&str>,
        servers: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            own_login: own_login
                .filter(|login| !login.is_empty())
                .map(str::to_owned),
            // `/` would turn every path into one starting with `~`.
            home: home.filter(|home| home.len() > 1).map(str::to_owned),
            servers: servers
                .into_iter()
                .filter(|server| !server.is_empty())
                .collect(),
            names: HashMap::new(),
        }
    }

    /// `line` with everything the module comment lists taken out.
    pub fn line(&mut self, line: &str) -> String {
        // Credentials first: a token can hold text that looks like an
        // address or a name.
        let mut text = AUTHORIZATION
            .replace_all(line, "$1$2[redacted]")
            .into_owned();
        text = BEARER.replace_all(&text, "$1 [redacted]").into_owned();
        text = SECRET_FIELD
            .replace_all(&text, "$1$2[redacted]")
            .into_owned();
        text = QUERY_SECRET.replace_all(&text, "$1[redacted]").into_owned();
        text = COOKIE.replace_all(&text, "$1$2[redacted]").into_owned();
        text = JWT.replace_all(&text, "[token]").into_owned();
        text = BLOB.replace_all(&text, "[blob]").into_owned();

        for server in &self.servers {
            text = text.replace(server.as_str(), "[server]");
        }
        if let Some(home) = &self.home {
            text = text.replace(home.as_str(), "~");
        }

        // An IRC prefix before e-mail addresses, which `user@host` looks like.
        text = IRC_PREFIX
            .replace_all(&text, |caps: &Captures| {
                format!(":{}", self.pseudonym(&caps[1]))
            })
            .into_owned();
        text = EMAIL.replace_all(&text, "[email]").into_owned();
        text = PRIVATE_V4.replace_all(&text, "[private]").into_owned();
        text = PRIVATE_V6.replace_all(&text, "[private]").into_owned();
        text = PRIVATE_HOST.replace_all(&text, "[private]").into_owned();

        text = NAME_LIST
            .replace_all(&text, |caps: &Captures| {
                let items = QUOTED.replace_all(&caps[2], |item: &Captures| {
                    format!("\"{}\"", self.pseudonym(&item[1]))
                });
                format!("{}=[{items}]", &caps[1])
            })
            .into_owned();
        text = NAME_FIELD
            .replace_all(&text, |caps: &Captures| {
                let value = &caps[2];
                match value
                    .strip_prefix('"')
                    .and_then(|inner| inner.strip_suffix('"'))
                {
                    Some(inner) => format!("{}=\"{}\"", &caps[1], self.pseudonym(inner)),
                    None => format!("{}={}", &caps[1], self.pseudonym(value)),
                }
            })
            .into_owned();
        PRIVATE_CHANNEL
            .replace_all(&text, |caps: &Captures| {
                format!("channel={}{}", &caps[1], self.pseudonym(&caps[2]))
            })
            .into_owned()
    }

    /// `name` as it appears in an export: the player's own login as it is,
    /// anyone else as `playerN`, numbered in the order they first appear.
    fn pseudonym(&mut self, name: &str) -> String {
        if self
            .own_login
            .as_deref()
            .is_some_and(|own| own.eq_ignore_ascii_case(name))
        {
            return name.to_owned();
        }
        let next = self.names.len() + 1;
        self.names
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| format!("player{next}"))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber() -> Scrubber {
        Scrubber::new(
            Some("Ada"),
            Some("/data/user/0/com.faforever.rustclient/files"),
            ["test.example.org".to_string()],
        )
    }

    #[test]
    fn another_players_login_becomes_a_number_and_the_players_own_stays() {
        let mut scrub = scrubber();
        let line = "2026-11-19T10:01:02.003Z  WARN faf_app::services::social: could not look up an account by login login=Stormlord error=not found";
        assert_eq!(
            scrub.line(line),
            "2026-11-19T10:01:02.003Z  WARN faf_app::services::social: could not look up an account by login login=player1 error=not found"
        );
        assert!(scrub.line("lookup login=Ada").ends_with("login=Ada"));
        // The same name is the same number, in any case, in a later line.
        assert!(scrub
            .line("lookup login=stormlord")
            .ends_with("login=player1"));
        assert!(scrub.line("lookup login=Zep").ends_with("login=player2"));
    }

    #[test]
    fn a_private_conversation_is_renamed_and_a_public_channel_stays() {
        let mut scrub = scrubber();
        assert_eq!(
            scrub.line("WARN faf_app::infra::irc_session: the chat server refused a channel command numeric=401 channel=Stormlord reason=No such nick/channel"),
            "WARN faf_app::infra::irc_session: the chat server refused a channel command numeric=401 channel=player1 reason=No such nick/channel"
        );
        assert_eq!(
            scrub.line("INFO faf_app::infra::irc: chat join requested channel=#aeolus"),
            "INFO faf_app::infra::irc: chat join requested channel=#aeolus"
        );
    }

    #[test]
    fn quoted_names_lists_and_irc_prefixes_are_renamed() {
        let mut scrub = scrubber();
        assert_eq!(
            scrub.line(r#"peer="Stormlord" joined"#),
            r#"peer="player1" joined"#
        );
        assert_eq!(
            scrub.line(r#"players=["Ada", "Stormlord", "Zep"] ready"#),
            r#"players=["Ada", "player1", "player2"] ready"#
        );
        assert_eq!(
            scrub.line(":Zep!zep@faf.chat.host PRIVMSG Ada :hi"),
            ":player2 PRIVMSG Ada :hi"
        );
    }

    #[test]
    fn credentials_are_redacted() {
        let mut scrub = scrubber();
        assert_eq!(
            scrub.line("request failed headers={\"authorization\": \"Bearer abc.def.ghi\"}"),
            "request failed headers={\"authorization\": \"[redacted]\"}"
        );
        assert_eq!(
            scrub.line("Authorization: Bearer 0123456789abcdef"),
            "Authorization: [redacted]"
        );
        assert_eq!(
            scrub.line(r#"token refresh failed body={"error":"invalid_grant","refresh_token":"ory_rt_secret"}"#),
            r#"token refresh failed body={"error":"invalid_grant","refresh_token":"[redacted]"}"#
        );
        assert_eq!(
            scrub.line("redirect received url=http://127.0.0.1:53211/?code=ory_ac_secret&state=abcdef&scope=openid"),
            "redirect received url=http://127.0.0.1:53211/?code=[redacted]&state=[redacted]&scope=openid"
        );
        assert_eq!(
            scrub.line("set-cookie: session=abc; Path=/"),
            "set-cookie: [redacted]"
        );
        assert_eq!(
            scrub.line("id eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiI0MiJ9.c2lnbmF0dXJlLXZhbHVl end"),
            "id [token] end"
        );
    }

    #[test]
    fn the_machine_proof_and_long_blobs_are_redacted() {
        let mut scrub = scrubber();
        let proof = "A".repeat(120);
        assert_eq!(
            scrub.line(&format!(
                r#"auth sent {{"command":"auth","unique_id":"{proof}"}}"#
            )),
            r#"auth sent {"command":"auth","unique_id":"[redacted]"}"#
        );
        assert_eq!(scrub.line(&format!("payload {proof}")), "payload [blob]");
    }

    #[test]
    fn private_addresses_test_servers_mail_and_home_are_hidden() {
        let mut scrub = scrubber();
        assert_eq!(
            scrub.line("connecting to 192.168.1.20:8001 and 100.101.102.103 and 10.0.0.7"),
            "connecting to [private]:8001 and [private] and [private]"
        );
        assert_eq!(
            scrub.line("peer at fd7a:115c:a1e0::1 and fe80::1"),
            "peer at [private] and [private]"
        );
        assert_eq!(
            scrub.line("host tower.tail1234.ts.net up"),
            "host [private] up"
        );
        assert_eq!(
            scrub.line("lobby url=wss://test.example.org/lobby"),
            "lobby url=wss://[server]/lobby"
        );
        assert_eq!(scrub.line("mail ada@example.com"), "mail [email]");
        assert_eq!(
            scrub.line("could not write settings.json at /data/user/0/com.faforever.rustclient/files/.config/x"),
            "could not write settings.json at ~/.config/x"
        );
    }

    #[test]
    fn what_a_report_needs_stays() {
        let mut scrub = scrubber();
        for line in [
            // Lines from a client log on Android, as written.
            "2026-11-19T00:15:07.544973Z  INFO faforever_rust_client_lib::diagnostics: client diagnostics initialized retained_files=7 max_file_bytes=10485760",
            "2026-11-19T00:15:07.629461Z  INFO faf_app::infra::streams: no Twitch application credentials configured (TWITCH_CLIENT_ID / TWITCH_CLIENT_SECRET); FAF live announcements are off. See docs/streams.md",
            "2026-11-19T00:15:08.620553Z  INFO faforever_rust_client_lib::commands: webview hydration snapshot bytes=16933 copy_seconds=0.0006629300187341869",
            "2026-11-19T00:17:07.803437Z  INFO faf_app::runtime::census: state census total_bytes=120616 slices=clientUpdate=103586 settings=5543 mapGenerator=1804 tourney=1469",
            "2026-11-19T00:15:08.136480Z  INFO faf_app::runtime::census: state census total_bytes=120290 slices=settings=5543",
            "game exited code=0 after 1200s",
            "stored digest=0f4a9c2e7b1d3f5a6c8e0b2d4f6a8c0e1b3d5f7a9c1e3b5d7f9a1c3e5b7d9f1a",
            "relay stun:eu.relay.faforever.com:3478 answered in 23 ms",
            "loopback http://127.0.0.1:53211 and http://localhost:5173",
            "installed mod uid=a1b2c3d4-0000-4000-8000-123456789abc name=Supreme Score Board",
        ] {
            assert_eq!(scrub.line(line), line);
        }
    }
}
