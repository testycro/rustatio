use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Write};
use std::sync::{OnceLock, RwLock};

/// Built-in client definitions, used as the seed when nothing has been saved
/// yet (or when every saved definition is invalid).
const BUILTIN_CLIENTS_JSON: &str = include_str!("clients.default.json");

/// Identifier of an emulated client (e.g. `"qbittorrent"`).
///
/// Clients are no longer a closed list: any [`ClientDefinition`] present in the
/// active definitions can be selected. The identifier serializes as a plain
/// string, so existing configs and APIs are unchanged. An identifier that no
/// longer exists (client deleted) is not an error: it resolves to the default
/// client when a [`ClientConfig`] is built.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientType(String);

/// Metadata about a torrent client for UI/API display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Machine-readable ID (e.g., "qbittorrent")
    pub id: String,
    /// Human-readable display name (e.g., "qBittorrent")
    pub name: String,
    /// Default version to use
    pub default_version: String,
    /// Available versions (newest first)
    pub versions: Vec<String>,
    /// Default port
    pub default_port: u16,
}

/// How the 4-character version code of the peer ID is derived from the version string.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionScheme {
    /// Remove the dots ("3.5.5" -> "355")
    Digits,
    /// Concatenate major, minor and patch ("5.2.0" -> "520")
    #[default]
    MajorMinorPatch,
    /// Major followed by the minor zero-padded to 2 digits ("4.1.3" -> "410")
    MajorMinorPad2,
}

/// Full, data-driven description of an emulated client.
///
/// The list of definitions is persisted next to `default_config` in the state
/// file: add an entry to add a client, remove one to delete it. When nothing is
/// saved (or no saved entry is valid) `clients.default.json` is used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientDefinition {
    /// Machine-readable ID: lowercase letters, digits, '-' or '_' (e.g. "qbittorrent")
    pub id: String,
    /// Human-readable display name
    pub name: String,
    /// Version used when none is requested
    pub default_version: String,
    /// Available versions (newest first)
    pub versions: Vec<String>,
    #[serde(default = "default_client_port")]
    pub default_port: u16,
    /// Client used when none is selected or when the selected one no longer exists.
    /// If several (or none) are flagged, the first flagged one (or the first client) wins.
    #[serde(default)]
    pub is_default: bool,
    /// Two-character client code used in the peer ID (e.g. "qB" -> "-qB5300-...")
    pub peer_id_code: String,
    /// User-Agent template, `{version}` is replaced by the emulated version
    pub user_agent: String,
    #[serde(default)]
    pub version_scheme: VersionScheme,
    /// Version code used when the version has too few components
    #[serde(default)]
    pub fallback_version_code: Option<String>,
    #[serde(default = "default_client_num_want")]
    pub num_want: u32,
    #[serde(default = "default_true")]
    pub supports_compact: bool,
    #[serde(default = "default_true")]
    pub supports_crypto: bool,
    #[serde(default = "default_http_version")]
    pub http_version: HttpVersion,
}

const fn default_client_port() -> u16 {
    6881
}

const fn default_client_num_want() -> u32 {
    200
}

const fn default_true() -> bool {
    true
}

const fn default_http_version() -> HttpVersion {
    HttpVersion::Http11
}

impl ClientDefinition {
    /// A definition is usable when it has everything needed to build a valid
    /// 20-character peer ID and a user agent.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(format!(
                "invalid client id '{}': use lowercase letters, digits, '-' or '_'",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return Err(format!("client '{}': name is empty", self.id));
        }
        if self.default_version.trim().is_empty() {
            return Err(format!("client '{}': default_version is empty", self.id));
        }
        if self.versions.is_empty() {
            return Err(format!("client '{}': versions is empty", self.id));
        }
        if self.peer_id_code.chars().count() != 2
            || !self.peer_id_code.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(format!(
                "client '{}': peer_id_code must be exactly 2 ASCII alphanumeric characters",
                self.id
            ));
        }
        if self.user_agent.trim().is_empty() {
            return Err(format!("client '{}': user_agent is empty", self.id));
        }
        Ok(())
    }

    pub fn info(&self) -> ClientInfo {
        ClientInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            default_version: self.default_version.clone(),
            versions: self.versions.clone(),
            default_port: self.default_port,
        }
    }

    /// Minimal definition used only if the embedded JSON is unreadable.
    fn placeholder(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            default_version: "1.0.0".to_string(),
            versions: vec!["1.0.0".to_string()],
            default_port: default_client_port(),
            is_default: true,
            peer_id_code: "XX".to_string(),
            user_agent: format!("{id}/{{version}}"),
            version_scheme: VersionScheme::default(),
            fallback_version_code: None,
            num_want: default_client_num_want(),
            supports_compact: true,
            supports_crypto: true,
            http_version: default_http_version(),
        }
    }

    fn version_code(&self, version: &str) -> String {
        let parts: Vec<&str> = version.split('.').collect();
        let fallback = || self.fallback_version_code.clone().unwrap_or_else(|| "0000".to_string());

        let code = match self.version_scheme {
            VersionScheme::Digits => version.replace('.', ""),
            VersionScheme::MajorMinorPatch => {
                if parts.len() >= 3 {
                    format!("{}{}{}", parts[0], parts[1], parts[2])
                } else {
                    fallback()
                }
            }
            VersionScheme::MajorMinorPad2 => {
                if parts.len() >= 2 {
                    format!("{}{}", parts[0], parts[1].pad_to_width_with_char(2, '0'))
                } else {
                    fallback()
                }
            }
        };

        // Pad to exactly 4 characters
        code.pad_to_width_with_char(4, '0')
    }

    /// Build the runtime configuration for this client at the given version.
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn to_config(&self, version: Option<String>) -> ClientConfig {
        let version = version.unwrap_or_else(|| self.default_version.clone());
        let padded_version = self.version_code(&version);

        ClientConfig {
            client_type: ClientType::new(self.id.clone()),
            peer_id_prefix: format!("-{}{padded_version}-", self.peer_id_code),
            user_agent: self.user_agent.replace("{version}", &version),
            version,
            http_version: self.http_version.clone(),
            num_want: self.num_want,
            supports_compact: self.supports_compact,
            supports_crypto: self.supports_crypto,
        }
    }
}

static BUILTIN_DEFINITIONS: OnceLock<Vec<ClientDefinition>> = OnceLock::new();
/// `None` means "use the built-in definitions".
static SAVED_DEFINITIONS: RwLock<Option<Vec<ClientDefinition>>> = RwLock::new(None);

/// Definitions shipped with the application (`clients.default.json`).
pub fn builtin_client_definitions() -> &'static [ClientDefinition] {
    BUILTIN_DEFINITIONS.get_or_init(|| {
        serde_json::from_str(BUILTIN_CLIENTS_JSON).unwrap_or_else(|e| {
            log::error!("Failed to parse built-in client definitions: {e}");
            Vec::new()
        })
    })
}

/// Replace the active client definitions (e.g. loaded from the state file).
///
/// Invalid entries are skipped with a warning (a later entry replaces an
/// earlier one with the same id). If nothing valid remains, the built-in
/// definitions are used. Returns the number of definitions accepted.
pub fn set_client_definitions(definitions: Vec<ClientDefinition>) -> usize {
    let mut accepted: Vec<ClientDefinition> = Vec::new();

    for definition in definitions {
        match definition.validate() {
            Ok(()) => {
                accepted.retain(|d| d.id != definition.id);
                accepted.push(definition);
            }
            Err(reason) => log::warn!("Ignoring invalid client definition: {reason}"),
        }
    }

    let count = accepted.len();
    if count == 0 {
        log::warn!("No valid client definition found, using the built-in ones");
    }

    if let Ok(mut saved) = SAVED_DEFINITIONS.write() {
        *saved = (count > 0).then_some(accepted);
    }
    count
}

/// Go back to the built-in definitions.
pub fn reset_client_definitions() {
    if let Ok(mut saved) = SAVED_DEFINITIONS.write() {
        *saved = None;
    }
}

/// Add a client, or replace the one with the same id.
pub fn upsert_client_definition(definition: ClientDefinition) -> Result<(), String> {
    update_definitions(|list| with_upserted(list, definition))
}

/// Delete a client. The last remaining client cannot be deleted.
pub fn remove_client_definition(id: &str) -> Result<(), String> {
    update_definitions(|list| without_client(list, id))
}

fn update_definitions<F>(change: F) -> Result<(), String>
where
    F: FnOnce(Vec<ClientDefinition>) -> Result<Vec<ClientDefinition>, String>,
{
    let mut saved = SAVED_DEFINITIONS.write().map_err(|_| "client registry unavailable")?;
    let current = saved.clone().unwrap_or_else(|| builtin_client_definitions().to_vec());
    *saved = Some(change(current)?);
    Ok(())
}

fn with_upserted(
    mut list: Vec<ClientDefinition>,
    definition: ClientDefinition,
) -> Result<Vec<ClientDefinition>, String> {
    definition.validate()?;

    if definition.is_default {
        for existing in &mut list {
            existing.is_default = false;
        }
    }

    match list.iter_mut().find(|d| d.id == definition.id) {
        Some(existing) => *existing = definition,
        None => list.push(definition),
    }
    Ok(list)
}

fn without_client(
    mut list: Vec<ClientDefinition>,
    id: &str,
) -> Result<Vec<ClientDefinition>, String> {
    let before = list.len();
    list.retain(|d| d.id != id);

    if list.len() == before {
        return Err(format!("client '{id}' not found"));
    }
    if list.is_empty() {
        return Err("the last client cannot be deleted".to_string());
    }
    Ok(list)
}

/// All definitions currently in effect (saved ones, else built-in), in display order.
/// This is what gets persisted next to `default_config`.
pub fn client_definitions() -> Vec<ClientDefinition> {
    if let Ok(saved) = SAVED_DEFINITIONS.read() {
        if let Some(definitions) = saved.as_ref() {
            return definitions.clone();
        }
    }
    builtin_client_definitions().to_vec()
}

/// Definition of the client with this id, if it exists.
pub fn client_definition(id: &str) -> Option<ClientDefinition> {
    client_definitions().into_iter().find(|d| d.id == id)
}

/// Client used when none is selected: the one flagged `is_default`, else the first.
pub fn default_client_definition() -> ClientDefinition {
    let mut definitions = client_definitions();
    let index = definitions.iter().position(|d| d.is_default).unwrap_or(0);

    if index < definitions.len() {
        definitions.swap_remove(index)
    } else {
        ClientDefinition::placeholder("qbittorrent")
    }
}

impl ClientType {
    /// Wrap an identifier. It is not checked against the definitions.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Machine-readable ID of this client
    pub fn id(&self) -> &str {
        &self.0
    }

    /// Get all available client types
    pub fn all() -> Vec<Self> {
        client_definitions().into_iter().map(|d| Self(d.id)).collect()
    }

    /// Get all client type IDs as strings
    pub fn all_ids() -> Vec<String> {
        client_definitions().into_iter().map(|d| d.id).collect()
    }

    /// Get all client infos
    pub fn all_infos() -> Vec<ClientInfo> {
        client_definitions().iter().map(ClientDefinition::info).collect()
    }

    /// Parse a client type from its string ID (only if such a client exists)
    pub fn from_id(id: &str) -> Option<Self> {
        client_definition(id).map(|d| Self(d.id))
    }

    /// Definition of this client; an unknown id resolves to the default client
    pub fn definition(&self) -> ClientDefinition {
        client_definition(&self.0).unwrap_or_else(|| {
            log::warn!("Unknown client '{}', falling back to the default client", self.0);
            default_client_definition()
        })
    }

    /// Get full metadata for this client type
    pub fn info(&self) -> ClientInfo {
        self.definition().info()
    }
}

impl Default for ClientType {
    fn default() -> Self {
        Self(default_client_definition().id)
    }
}

impl fmt::Display for ClientType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub client_type: ClientType,
    pub version: String,
    pub peer_id_prefix: String,
    pub user_agent: String,
    pub http_version: HttpVersion,
    pub num_want: u32,
    pub supports_compact: bool,
    pub supports_crypto: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpVersion {
    Http10,
    Http11,
}

impl ClientConfig {
    /// Get configuration for a specific client, built from its active definition.
    ///
    /// When the client no longer exists, the default client is used at its own default
    /// version: the requested version belongs to the deleted client and would give the
    /// default client a peer ID and User-Agent that never existed.
    pub fn get(client_type: &ClientType, version: Option<String>) -> Self {
        client_definition(client_type.id()).map_or_else(
            || client_type.definition().to_config(None),
            |definition| definition.to_config(version),
        )
    }

    /// Generate a random peer ID based on this client config
    pub fn generate_peer_id(&self) -> String {
        let mut rng = rand::rng();
        let random_suffix: String = (0..12)
            .map(|_| {
                let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
                chars[rng.random_range(0..chars.len())] as char
            })
            .collect();

        format!("{}{}", self.peer_id_prefix, random_suffix)
    }

    /// Generate a random key (8 hex characters)
    pub fn generate_key() -> String {
        let mut rng = rand::rng();
        (0..8).fold(String::new(), |mut acc, _| {
            let _ = write!(acc, "{:X}", rng.random_range(0..16));
            acc
        })
    }
}

trait PadString {
    fn pad_to_width_with_char(&self, width: usize, ch: char) -> String;
}

impl PadString for str {
    /// Works on characters: slicing bytes panicked (and aborted the whole process, see
    /// `panic = "abort"`) on a version holding a non-ASCII character.
    fn pad_to_width_with_char(&self, width: usize, ch: char) -> String {
        let mut padded: String = self.chars().take(width).collect();
        let len = padded.chars().count();
        padded.extend(std::iter::repeat_n(ch, width - len));
        padded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value};
    use std::collections::HashSet;

    // `clients.default.json` is the single source of truth. No client, version or
    // value is hard-coded in the tests below: every expectation is read from the
    // file, and the file itself is checked against a strict schema so that a bad
    // edit fails CI instead of shipping.

    /// Client that must always be shipped (it is also the last-resort placeholder).
    const MANDATORY_CLIENT: &str = "qbittorrent";

    /// Collects every problem so that a single run lists all of them.
    #[derive(Default)]
    struct Report(Vec<String>);

    impl Report {
        fn fail(&mut self, problem: String) {
            self.0.push(problem);
        }

        fn check(&mut self, ok: bool, problem: impl FnOnce() -> String) {
            if !ok {
                self.fail(problem());
            }
        }

        fn assert_clean(&self) {
            assert!(
                self.0.is_empty(),
                "clients.default.json is invalid:\n  - {}",
                self.0.join("\n  - ")
            );
        }
    }

    /// Raw entries of the file, exactly as written (no serde default applied).
    fn raw_entries() -> Vec<Map<String, Value>> {
        let root: Value = serde_json::from_str(BUILTIN_CLIENTS_JSON)
            .unwrap_or_else(|e| panic!("clients.default.json is not valid JSON: {e}"));
        let Value::Array(entries) = root else {
            panic!("clients.default.json must be a JSON array");
        };
        entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| match entry {
                Value::Object(map) => map,
                other => panic!("entry #{index} must be a JSON object, got: {other}"),
            })
            .collect()
    }

    /// Definitions parsed from the file. Parsed here rather than through
    /// `builtin_client_definitions`, which silently returns an empty list on error,
    /// and without going through the global registry, which other tests may change.
    fn definitions() -> Vec<ClientDefinition> {
        serde_json::from_str(BUILTIN_CLIENTS_JSON).unwrap_or_else(|e| {
            panic!("clients.default.json does not match `ClientDefinition`: {e}")
        })
    }

    /// Any client id present in the file.
    fn any_id() -> String {
        definitions().into_iter().next().map_or_else(String::new, |d| d.id)
    }

    /// Id of the client flagged `is_default` in the file.
    fn json_default_id() -> String {
        definitions().into_iter().find(|d| d.is_default).map_or_else(String::new, |d| d.id)
    }

    /// Numeric components of a version ("4.1.3" -> [4, 1, 3]), `None` when malformed.
    fn version_numbers(version: &str) -> Option<Vec<u64>> {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() < 2 {
            return None;
        }
        parts
            .iter()
            .map(|part| {
                let digits = !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
                digits.then(|| part.parse().ok()).flatten()
            })
            .collect()
    }

    /// Independent (test-side) computation of the 4-character version code.
    fn expected_version_code(definition: &ClientDefinition, version: &str) -> String {
        let parts: Vec<&str> = version.split('.').collect();
        let fallback =
            || definition.fallback_version_code.clone().unwrap_or_else(|| "0000".to_string());
        let code = match definition.version_scheme {
            VersionScheme::Digits => version.replace('.', ""),
            VersionScheme::MajorMinorPad2 if parts.len() >= 2 => {
                let minor: String = format!("{:0<2}", parts[1]).chars().take(2).collect();
                format!("{}{minor}", parts[0])
            }
            VersionScheme::MajorMinorPatch if parts.len() >= 3 => {
                format!("{}{}{}", parts[0], parts[1], parts[2])
            }
            _ => fallback(),
        };
        format!("{code:0<4}").chars().take(4).collect()
    }

    fn expected_prefix(definition: &ClientDefinition, version: &str) -> String {
        format!("-{}{}-", definition.peer_id_code, expected_version_code(definition, version))
    }

    // ---------------------------------------------------------------------
    // Strict schema of clients.default.json
    // ---------------------------------------------------------------------

    #[test]
    fn test_json_parses_and_is_the_builtin_list() {
        let definitions = definitions();
        assert!(!definitions.is_empty(), "clients.default.json defines no client");
        assert_eq!(definitions, builtin_client_definitions());
    }

    /// Every field of `ClientDefinition` must be written explicitly (a missing key
    /// would be silently replaced by a serde default), no unknown key is allowed,
    /// and every value must read back unchanged.
    #[test]
    fn test_json_has_every_key_and_no_unknown_key() {
        let mut report = Report::default();
        for (index, (entry, definition)) in raw_entries().iter().zip(&definitions()).enumerate() {
            let Value::Object(expected) = serde_json::to_value(definition).unwrap() else {
                unreachable!("a ClientDefinition always serializes to an object")
            };
            let label = format!("entry #{index} ({})", definition.id);
            for key in expected.keys() {
                report.check(entry.contains_key(key), || format!("{label}: missing key '{key}'"));
            }
            for (key, value) in entry {
                match expected.get(key) {
                    None => report.fail(format!("{label}: unknown key '{key}'")),
                    Some(read) => report.check(read == value, || {
                        format!("{label}: '{key}' is {value} but is read as {read}")
                    }),
                }
            }
        }
        report.assert_clean();
    }

    #[test]
    fn test_json_has_one_default_client_and_the_mandatory_one() {
        let definitions = definitions();
        let mut report = Report::default();

        let defaults: Vec<&str> =
            definitions.iter().filter(|d| d.is_default).map(|d| d.id.as_str()).collect();
        report.check(defaults.len() == 1, || {
            format!("exactly one client must have \"is_default\": true, found {defaults:?}")
        });
        report.check(definitions.iter().any(|d| d.id == MANDATORY_CLIENT), || {
            format!("client '{MANDATORY_CLIENT}' is mandatory")
        });

        let ids: Vec<String> = definitions.iter().map(|d| d.id.clone()).collect();
        let names: Vec<String> = definitions.iter().map(|d| d.name.to_lowercase()).collect();
        let codes: Vec<String> = definitions.iter().map(|d| d.peer_id_code.clone()).collect();
        report_duplicates(&mut report, "id", &ids);
        report_duplicates(&mut report, "name", &names);
        report_duplicates(&mut report, "peer_id_code", &codes);
        report.assert_clean();
    }

    fn report_duplicates(report: &mut Report, field: &str, values: &[String]) {
        let mut seen = HashSet::new();
        for value in values {
            report.check(seen.insert(value), || format!("duplicate {field} '{value}'"));
        }
    }

    #[test]
    fn test_json_every_client_is_well_formed() {
        let mut report = Report::default();
        for definition in &definitions() {
            if let Err(reason) = definition.validate() {
                report.fail(reason);
            }
            check_text_fields(&mut report, definition);
            check_versions(&mut report, definition);
            check_numbers(&mut report, definition);
            check_fallback(&mut report, definition);
        }
        report.assert_clean();
    }

    #[allow(clippy::literal_string_with_formatting_args)]
    fn check_text_fields(report: &mut Report, definition: &ClientDefinition) {
        let id = &definition.id;
        let agent = &definition.user_agent;
        report.check(definition.name.trim() == definition.name, || {
            format!("{id}: name has leading or trailing spaces")
        });
        report.check(agent.matches("{version}").count() == 1, || {
            format!("{id}: user_agent must contain {{version}} exactly once")
        });
        report.check(
            agent.trim() == agent.as_str()
                && agent.chars().all(|c| c.is_ascii_graphic() || c == ' '),
            || format!("{id}: user_agent must be trimmed printable ASCII (HTTP header)"),
        );
    }

    fn check_versions(report: &mut Report, definition: &ClientDefinition) {
        let id = &definition.id;
        report.check(definition.versions.first() == Some(&definition.default_version), || {
            format!("{id}: default_version must be the first (newest) entry of versions")
        });
        let mut previous: Option<Vec<u64>> = None;
        for version in &definition.versions {
            let Some(numbers) = version_numbers(version) else {
                report.fail(format!("{id}: version '{version}' must be numbers separated by dots"));
                continue;
            };
            if let Some(newer) = &previous {
                report.check(numbers.as_slice() < newer.as_slice(), || {
                    format!(
                        "{id}: versions must be unique and newest first ('{version}' is misplaced)"
                    )
                });
            }
            previous = Some(numbers);
        }
    }

    fn check_numbers(report: &mut Report, definition: &ClientDefinition) {
        let id = &definition.id;
        report.check(definition.default_port >= 1024, || {
            format!("{id}: default_port {} is a privileged port", definition.default_port)
        });
        report.check(definition.num_want > 0, || format!("{id}: num_want must be at least 1"));
    }

    fn check_fallback(report: &mut Report, definition: &ClientDefinition) {
        let id = &definition.id;
        let scheme = definition.version_scheme;
        match (&definition.fallback_version_code, scheme) {
            (None, VersionScheme::Digits) => {}
            (Some(_), VersionScheme::Digits) => {
                report.fail(format!(
                    "{id}: fallback_version_code is unused with {scheme:?}, use null"
                ));
            }
            (None, _) => {
                report.fail(format!("{id}: fallback_version_code is required with {scheme:?}"));
            }
            (Some(code), _) => report.check(
                (1..=4).contains(&code.len()) && code.chars().all(|c| c.is_ascii_digit()),
                || format!("{id}: fallback_version_code '{code}' must be 1 to 4 digits"),
            ),
        }
    }

    // ---------------------------------------------------------------------
    // Behaviour, driven by every client and every version of the file
    // ---------------------------------------------------------------------

    #[test]
    fn test_json_peer_id_for_every_client_and_version() {
        for definition in &definitions() {
            let requested =
                std::iter::once(None).chain(definition.versions.iter().cloned().map(Some));
            for version in requested {
                let config = definition.to_config(version.clone());
                let used = version.unwrap_or_else(|| definition.default_version.clone());
                let label = format!("{} {used}", definition.id);
                let prefix = &config.peer_id_prefix;

                assert_eq!(config.version, used, "{label}");
                assert_eq!(*prefix, expected_prefix(definition, &used), "{label}");
                assert_eq!(prefix.chars().count(), 8, "{label}: {prefix}");
                assert!(
                    prefix.starts_with('-')
                        && prefix.ends_with('-')
                        && prefix.chars().skip(1).take(6).all(|c| c.is_ascii_alphanumeric()),
                    "{label}: malformed peer ID prefix {prefix}"
                );

                let peer_id = config.generate_peer_id();
                assert_eq!(peer_id.len(), 20, "{label}");
                assert!(peer_id.is_ascii(), "{label}");
            }
        }
    }

    #[test]
    #[allow(clippy::literal_string_with_formatting_args)]
    fn test_json_client_config_for_every_client() {
        for definition in &definitions() {
            let config = definition.to_config(None);
            let label = &definition.id;
            let agent = definition.user_agent.replacen("{version}", &definition.default_version, 1);

            assert_eq!(config.client_type, ClientType::new(definition.id.clone()), "{label}");
            assert_eq!(config.version, definition.default_version, "{label}");
            assert_eq!(config.user_agent, agent, "{label}");
            assert_eq!(config.num_want, definition.num_want, "{label}");
            assert_eq!(config.supports_compact, definition.supports_compact, "{label}");
            assert_eq!(config.supports_crypto, definition.supports_crypto, "{label}");
            assert_eq!(config.http_version, definition.http_version, "{label}");
        }
    }

    #[test]
    fn test_registry_serves_the_json_definitions() {
        let definitions = definitions();
        let ids: Vec<String> = definitions.iter().map(|d| d.id.clone()).collect();
        assert_eq!(ClientType::all_ids(), ids);

        for definition in &definitions {
            let client = ClientType::new(definition.id.clone());
            assert_eq!(ClientType::from_id(&definition.id), Some(client.clone()));

            let info = client.info();
            assert_eq!(info.name, definition.name);
            assert_eq!(info.default_version, definition.default_version);
            assert_eq!(info.versions, definition.versions);
            assert_eq!(info.default_port, definition.default_port);

            let config = ClientConfig::get(&client, None);
            assert_eq!(config.peer_id_prefix, definition.to_config(None).peer_id_prefix);
        }
        assert_eq!(default_client_definition().id, json_default_id());
    }

    // ---------------------------------------------------------------------
    // Self-contained checks (custom definitions, independent of the file)
    // ---------------------------------------------------------------------

    #[test]
    fn test_client_config_with_custom_version() {
        let config = ClientConfig::get(&ClientType::new(any_id()), Some("9.8.7".to_string()));
        assert_eq!(config.version, "9.8.7");
        assert!(config.user_agent.contains("9.8.7"));
    }

    #[test]
    fn test_definition_drives_config() {
        let mut definition = custom("my-client");
        definition.peer_id_code = "zZ".to_string();
        definition.user_agent = "Custom/{version} (test)".to_string();
        definition.num_want = 42;

        let config = definition.to_config(Some("5.2.0".to_string()));
        assert_eq!(config.client_type, ClientType::new("my-client"));
        assert_eq!(config.peer_id_prefix, "-zZ5200-");
        assert_eq!(config.user_agent, "Custom/5.2.0 (test)");
        assert_eq!(config.num_want, 42);
        assert_eq!(config.generate_peer_id().len(), 20);
    }

    #[test]
    fn test_version_scheme_fallbacks() {
        let mut definition = custom("my-client");

        definition.version_scheme = VersionScheme::MajorMinorPatch;
        definition.fallback_version_code = Some("0161".to_string());
        assert_eq!(definition.to_config(Some("1".to_string())).peer_id_prefix, "-CU0161-");

        definition.version_scheme = VersionScheme::MajorMinorPad2;
        definition.fallback_version_code = None;
        assert_eq!(definition.to_config(Some("4".to_string())).peer_id_prefix, "-CU0000-");

        definition.version_scheme = VersionScheme::Digits;
        assert_eq!(definition.to_config(Some("4".to_string())).peer_id_prefix, "-CU4000-");
    }

    #[test]
    fn test_json_checks_catch_bad_entries() {
        // Guards the guards: the strict checks must reject typical mistakes.
        let mut report = Report::default();
        let mut definition = custom("my-client");
        definition.versions = vec!["1.2.3".to_string(), "1.10.0".to_string(), "x.1".to_string()];
        definition.user_agent = "Custom".to_string();
        definition.default_port = 80;
        definition.num_want = 0;
        check_versions(&mut report, &definition);
        check_text_fields(&mut report, &definition);
        check_numbers(&mut report, &definition);
        check_fallback(&mut report, &definition);
        // misplaced 1.10.0, malformed x.1, missing {version}, port, num_want, fallback
        assert_eq!(report.0.len(), 6, "{:?}", report.0);

        let mut report = Report::default();
        let names = ["a".to_string(), "b".to_string(), "a".to_string()];
        report_duplicates(&mut report, "id", &names);
        assert_eq!(report.0.len(), 1);
    }

    #[test]
    fn test_peer_id_uniqueness() {
        let config = ClientConfig::get(&ClientType::new(any_id()), None);
        let peer_id1 = config.generate_peer_id();
        let peer_id2 = config.generate_peer_id();

        // Peer IDs should be different (random suffixes)
        assert_ne!(peer_id1, peer_id2, "Generated peer IDs should be unique");
    }

    #[test]
    fn test_peer_id_valid_characters() {
        let config = ClientConfig::get(&ClientType::new(any_id()), None);
        let peer_id = config.generate_peer_id();

        // All characters should be valid (alphanumeric or -)
        assert!(peer_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn test_key_generation() {
        let key = ClientConfig::generate_key();
        assert_eq!(key.len(), 8, "Key must be exactly 8 characters");
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()), "Key must be hexadecimal");
    }

    #[test]
    fn test_key_uniqueness() {
        let key1 = ClientConfig::generate_key();
        let key2 = ClientConfig::generate_key();

        // Keys should be different (random)
        assert_ne!(key1, key2, "Generated keys should be unique");
    }

    #[test]
    fn test_key_uppercase() {
        let key = ClientConfig::generate_key();
        // All hex digits should be uppercase
        assert!(key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
    }

    #[test]
    fn test_pad_string_trait() {
        assert_eq!("12".pad_to_width_with_char(4, '0'), "1200");
        assert_eq!("1234".pad_to_width_with_char(4, '0'), "1234");
        assert_eq!("12345".pad_to_width_with_char(4, '0'), "1234");
        assert_eq!("1".pad_to_width_with_char(3, 'x'), "1xx");
    }

    fn any_def() -> ClientDefinition {
        definitions().into_iter().next().unwrap_or_else(|| unreachable!("no client in the JSON"))
    }

    fn custom(id: &str) -> ClientDefinition {
        ClientDefinition {
            id: id.to_string(),
            name: "Custom".to_string(),
            default_version: "1.2.3".to_string(),
            versions: vec!["1.2.3".to_string()],
            default_port: 7000,
            is_default: false,
            peer_id_code: "CU".to_string(),
            user_agent: "Custom/{version}".to_string(),
            version_scheme: VersionScheme::MajorMinorPatch,
            fallback_version_code: None,
            num_want: 10,
            supports_compact: true,
            supports_crypto: false,
            http_version: HttpVersion::Http11,
        }
    }

    #[test]
    fn test_validate_rejects_bad_definitions() {
        assert!(any_def().validate().is_ok());

        let mut definition = any_def();
        definition.peer_id_code = "DEL".to_string();
        assert!(definition.validate().is_err());

        let mut definition = any_def();
        definition.id = "Not Valid".to_string();
        assert!(definition.validate().is_err());

        let mut definition = any_def();
        definition.id = String::new();
        assert!(definition.validate().is_err());

        let mut definition = any_def();
        definition.versions.clear();
        assert!(definition.validate().is_err());
    }

    #[test]
    fn test_with_upserted_adds_and_replaces() {
        let list = builtin_client_definitions().to_vec();
        let count = list.len();

        let list = with_upserted(list, custom("my-client")).unwrap_or_else(|_| unreachable!());
        assert_eq!(list.len(), count + 1);

        let mut changed = custom("my-client");
        changed.name = "Renamed".to_string();
        let list = with_upserted(list, changed).unwrap_or_else(|_| unreachable!());
        assert_eq!(list.len(), count + 1);
        assert_eq!(list.iter().filter(|d| d.id == "my-client").count(), 1);
        assert!(list.iter().any(|d| d.name == "Renamed"));

        assert!(with_upserted(list, custom("Bad Id")).is_err());
    }

    #[test]
    fn test_with_upserted_default_is_exclusive() {
        let mut new_default = custom("my-client");
        new_default.is_default = true;

        let list = with_upserted(builtin_client_definitions().to_vec(), new_default)
            .unwrap_or_else(|_| unreachable!());
        let defaults: Vec<_> =
            list.iter().filter(|d| d.is_default).map(|d| d.id.as_str()).collect();
        assert_eq!(defaults, vec!["my-client"]);
    }

    #[test]
    fn test_without_client() {
        let list = builtin_client_definitions().to_vec();
        let count = list.len();

        let list = without_client(list, &any_id()).unwrap_or_else(|_| unreachable!());
        assert_eq!(list.len(), count - 1);
        assert!(list.iter().all(|d| d.id != any_id()));

        assert!(without_client(list, &any_id()).is_err());

        let single = vec![custom("only-one")];
        assert!(without_client(single, "only-one").is_err());
    }

    #[test]
    fn test_client_type_serializes_as_plain_string() {
        let id = any_id();
        let client = ClientType::new(id.clone());
        assert_eq!(serde_json::to_string(&client).unwrap_or_default(), format!("\"{id}\""));

        // Unknown ids must not break deserialization of saved configs
        let parsed: ClientType =
            serde_json::from_str("\"deleted-client\"").unwrap_or_else(|_| unreachable!());
        assert_eq!(parsed.id(), "deleted-client");
    }

    #[test]
    fn test_unknown_client_falls_back_to_default() {
        let config = ClientConfig::get(&ClientType::new("deleted-client"), None);
        assert_eq!(config.client_type, ClientType::default());
        assert_eq!(ClientType::default(), ClientType::new(json_default_id()));
    }

    #[test]
    fn test_unknown_client_ignores_the_version_of_the_deleted_client() {
        let config = ClientConfig::get(
            &ClientType::new("deleted-client"),
            Some("0.0.0-deleted".to_string()),
        );
        let default = default_client_definition();
        assert_eq!(config.client_type, ClientType::new(default.id.clone()));
        assert_eq!(config.version, default.default_version);
    }

    #[test]
    fn test_version_code_with_non_ascii_characters_does_not_panic() {
        let config = ClientConfig::get(&ClientType::new(any_id()), Some("abcé.1".to_string()));
        assert_eq!(config.peer_id_prefix.chars().count(), 8);

        assert_eq!("abcé1".pad_to_width_with_char(4, '0'), "abcé");
        assert_eq!("é".pad_to_width_with_char(2, '0'), "é0");
        assert_eq!("12".pad_to_width_with_char(4, '0'), "1200");
    }

    #[test]
    fn test_definitions_json_round_trip_with_defaults() {
        let json = r#"{
            "id": "deluge", "name": "Deluge", "default_version": "2.2.0",
            "versions": ["2.2.0"], "peer_id_code": "DE", "user_agent": "Deluge/{version}"
        }"#;
        let definition: ClientDefinition =
            serde_json::from_str(json).unwrap_or_else(|_| unreachable!());
        assert_eq!(definition.default_port, 6881);
        assert_eq!(definition.num_want, 200);
        assert!(!definition.is_default);
        assert_eq!(definition.http_version, HttpVersion::Http11);
        assert_eq!(definition.version_scheme, VersionScheme::MajorMinorPatch);
    }
}
