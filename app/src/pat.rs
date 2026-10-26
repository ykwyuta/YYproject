//! Personal access tokens are printed once; only SHA-256 digests are persisted.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, io::Write, path::Path};
use subtle::ConstantTimeEq;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerSettings {
    pub allowed_hosts: Vec<String>,
    pub allowed_origins: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct User {
    pub name: String,
    pub pat_sha256: String,
    #[serde(default)]
    pub read_only: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerSettings,
    pub users: Vec<User>,
}
pub fn digest(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn random_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "OS random generator unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if fs::metadata(path)?.len() > 1024 * 1024 {
            return Err("PAT config exceeds 1 MiB".into());
        }
        let config: Self = toml::from_str(&fs::read_to_string(path)?)?;
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<()> {
        if self.users.is_empty() || self.users.len() > 256 {
            return Err("PAT config requires 1..256 users".into());
        }
        let mut names = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        for user in &self.users {
            if user.name.trim().is_empty()
                || user.name.len() > 128
                || user.name.chars().any(char::is_control)
                || !names.insert(&user.name)
                || !hashes.insert(&user.pat_sha256)
                || user.pat_sha256.len() != 64
                || !user
                    .pat_sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err("Invalid or duplicate PAT user/digest".into());
            }
        }
        Ok(())
    }
    pub fn authenticate(&self, token: &str) -> Option<User> {
        if !token.starts_with("yyp_") || token.len() != 68 {
            return None;
        }
        let hash = digest(token);
        self.users
            .iter()
            .find(|u| bool::from(u.pat_sha256.as_bytes().ct_eq(hash.as_bytes())))
            .cloned()
    }
}
/// An exclusive sidecar lock prevents concurrent token generation losing another user.
pub fn generate(path: &Path, name: &str, read_only: bool) -> Result<String> {
    let lock_path = path.with_extension("toml.lock");
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)?;
    struct Unlock {
        path: std::path::PathBuf,
        file: Option<fs::File>,
    }
    impl Drop for Unlock {
        fn drop(&mut self) {
            drop(self.file.take());
            let _ = fs::remove_file(&self.path);
        }
    }
    let unlock = Unlock {
        path: lock_path,
        file: Some(lock),
    };
    let mut config = if path.exists() {
        Config::load(path)?
    } else {
        Config::default()
    };
    let token = format!("yyp_{}", random_id()?);
    config.users.retain(|u| u.name != name);
    config.users.push(User {
        name: name.into(),
        pat_sha256: digest(&token),
        read_only,
    });
    config.validate()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temp.write_all(toml::to_string_pretty(&config)?.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    drop(unlock);
    Ok(token)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_hashes_rotates_and_preserves_other_users() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.toml");
        let first = generate(&path, "alice", false).unwrap();
        let bob = generate(&path, "bob", true).unwrap();
        let second = generate(&path, "alice", false).unwrap();
        let config = Config::load(&path).unwrap();
        assert!(config.authenticate(&first).is_none());
        assert_eq!(config.authenticate(&second).unwrap().name, "alice");
        assert!(config.authenticate(&bob).unwrap().read_only);
        let text = fs::read_to_string(&path).unwrap();
        for token in [first, bob, second] {
            assert!(!text.contains(&token));
        }
        assert_eq!(config.users.len(), 2);
    }
    #[test]
    fn failed_generation_releases_lock_and_does_not_replace_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.toml");
        generate(&path, "alice", false).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(generate(&path, "", false).is_err());
        assert!(!path.with_extension("toml.lock").exists());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(path.with_extension("toml.lock"), "owned by another writer").unwrap();
        assert!(generate(&path, "bob", false).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
