//! Private, versioned local state. Nothing here writes the user's document.
use crate::{app::Message, document::Recovery, tools::Grant};
use anyhow::{Context, Result, bail};
use genai::chat::ChatMessage;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_STATE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub schema_version: u32,
    pub messages: Vec<Message>,
    pub history: Vec<ChatMessage>,
    pub model: String,
    pub agent: String,
    pub workspace: PathBuf,
    #[serde(default)]
    pub incomplete: bool,
}
#[derive(Serialize, Deserialize)]
struct RecoveryFile {
    schema_version: u32,
    document: Recovery,
}
#[derive(Serialize, Deserialize)]
struct GrantsFile {
    schema_version: u32,
    grants: Vec<Grant>,
}

pub struct Store {
    pub root: PathBuf,
    instance: String,
}

pub struct RecoveryEntry {
    pub id: String,
    pub label: String,
}
impl Store {
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            root,
            instance: uuid::Uuid::new_v4().to_string(),
        })
    }
    pub fn recovery_id(&self) -> &str {
        &self.instance
    }
    /// Independent pre-agent checkpoint, retained across normal exit and Keep.
    pub fn agent_checkpoint(&self, id: &str, document: Recovery) -> Result<()> {
        write_json(
            &self.path("recovery", id)?,
            &RecoveryFile {
                schema_version: 1,
                document,
            },
        )
    }
    pub fn checkpoint(
        &self,
        recovery: Option<Recovery>,
        session: Option<(&str, Session)>,
    ) -> Result<()> {
        let path = self.root.join(format!("recovery-{}.json", self.instance));
        if let Some(document) = recovery {
            write_json(
                &path,
                &RecoveryFile {
                    schema_version: 1,
                    document,
                },
            )?;
        } else {
            remove_if_present(&path)?;
        }
        if let Some((id, session)) = session {
            write_json(&self.path("session", id)?, &session)?;
        }
        Ok(())
    }
    pub fn finish(&self) -> Result<()> {
        remove_if_present(&self.root.join(format!("recovery-{}.json", self.instance)))
    }
    pub fn list(&self, kind: &str) -> Result<Vec<String>> {
        if !matches!(kind, "recovery" | "session") {
            bail!("Unknown state kind");
        }
        let prefix = format!("{kind}-");
        let mut ids = vec![];
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".json"))
                && uuid::Uuid::parse_str(id).is_ok()
            {
                ids.push(id.to_owned());
            }
        }
        ids.sort();
        Ok(ids)
    }
    pub fn recover(&self, id: &str) -> Result<Recovery> {
        let record: RecoveryFile = read_json(&self.path("recovery", id)?)?;
        if record.schema_version != 1 {
            bail!("Unsupported recovery version");
        }
        Ok(record.document)
    }
    /// Prepare picker metadata here, never during rendering. Newest snapshots come first.
    pub fn recovery_entries(&self) -> Result<Vec<RecoveryEntry>> {
        let mut entries = Vec::new();
        for id in self.list("recovery")? {
            let modified = fs::metadata(self.path("recovery", &id)?)?
                .modified()
                .unwrap_or(std::time::UNIX_EPOCH);
            let age = modified.elapsed().unwrap_or_default().as_secs();
            let age = if age < 60 {
                format!("{age}s")
            } else if age < 3600 {
                format!("{}m", age / 60)
            } else if age < 86400 {
                format!("{}h", age / 3600)
            } else {
                format!("{}d", age / 86400)
            };
            let name = match self.recover(&id) {
                Ok(record) => record
                    .path
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "[Untitled]".into()),
                Err(_) => "[Unavailable snapshot]".into(),
            };
            let label = format!("{} · {age} ago · {name}", &id[..8]);
            entries.push((modified, RecoveryEntry { id, label }));
        }
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        Ok(entries.into_iter().map(|(_, entry)| entry).collect())
    }
    pub fn resume(&self, id: &str) -> Result<Session> {
        let session: Session = read_json(&self.path("session", id)?)?;
        if session.schema_version != 1 {
            bail!("Unsupported session version");
        }
        Ok(session)
    }
    pub fn delete(&self, kind: &str, id: &str) -> Result<()> {
        remove_if_present(&self.path(kind, id)?)
    }
    pub fn grants(&self) -> Result<Vec<Grant>> {
        let path = self.root.join("permissions.json");
        if !path.exists() {
            return Ok(vec![]);
        }
        let record: GrantsFile = read_json(&path)?;
        if record.schema_version != 1 {
            bail!("Unsupported permission version");
        }
        Ok(record.grants)
    }
    pub fn save_grants(&self, grants: &[Grant]) -> Result<()> {
        write_json(
            &self.root.join("permissions.json"),
            &GrantsFile {
                schema_version: 1,
                grants: grants.to_vec(),
            },
        )
    }
    fn path(&self, kind: &str, id: &str) -> Result<PathBuf> {
        if !matches!(kind, "recovery" | "session") || uuid::Uuid::parse_str(id).is_err() {
            bail!("Invalid state identifier");
        }
        Ok(self.root.join(format!("{kind}-{id}.json")))
    }
}

pub fn state_dir() -> PathBuf {
    #[cfg(windows)]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .map(|home| home.join("AppData/Local"))
            })
            .unwrap_or_else(|| PathBuf::from("."))
            .join("queditor");
    }
    #[cfg(not(windows))]
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".local/state")
        })
        .join("queditor")
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("Local state exceeds 32 MiB limit");
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing state directory")?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("State symlinks are not supported");
    }
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("Local state exceeds size limit");
    }
    serde_json::from_slice(&bytes).context("Invalid local state JSON")
}
fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_does_not_write_document_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("state")).unwrap();
        let mut d = crate::document::Document::default();
        d.insert("unsaved");
        store.checkpoint(Some(d.recovery()), None).unwrap();
        let restored =
            crate::document::Document::from_recovery(store.recover(store.recovery_id()).unwrap())
                .unwrap();
        assert_eq!(restored.text, "unsaved");
        assert!(restored.dirty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = store.path("recovery", store.recovery_id()).unwrap();
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        store.finish().unwrap();
        assert!(store.list("recovery").unwrap().is_empty());
    }
    #[test]
    fn rejects_traversal_ids_and_only_persists_saved_grants() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("state")).unwrap();
        assert!(store.recover("../../secret").is_err());
        assert!(store.delete("other", "x").is_err());
        let grant = Grant {
            workspace: dir.path().into(),
            path: dir.path().join("note"),
            operation: "read_file".into(),
        };
        store.save_grants(std::slice::from_ref(&grant)).unwrap();
        assert_eq!(store.grants().unwrap(), vec![grant]);
    }
    #[test]
    fn sessions_round_trip_without_document_or_temporary_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("state")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let session = Session {
            schema_version: 1,
            messages: vec![Message {
                role: "you".into(),
                text: "prompt".into(),
                agent: None,
                model: None,
                usage: None,
            }],
            history: vec![ChatMessage::user("prompt")],
            model: "mock".into(),
            agent: "Planner".into(),
            workspace: dir.path().into(),
            incomplete: false,
        };
        store.checkpoint(None, Some((&id, session))).unwrap();
        let restored = store.resume(&id).unwrap();
        assert_eq!(restored.messages[0].text, "prompt");
        assert_eq!(restored.history.len(), 1);
        assert!(store.grants().unwrap().is_empty());
        assert!(store.list("recovery").unwrap().is_empty());
        store.delete("session", &id).unwrap();
        assert!(store.list("session").unwrap().is_empty());
    }

    #[test]
    fn recovery_picker_metadata_is_newest_first_and_tolerates_invalid_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("state")).unwrap();
        let older = uuid::Uuid::new_v4().to_string();
        let newer = uuid::Uuid::new_v4().to_string();
        let mut document = crate::document::Document::default();
        document.insert("private contents must not appear in labels");
        store.agent_checkpoint(&older, document.recovery()).unwrap();
        store.agent_checkpoint(&newer, document.recovery()).unwrap();
        fs::File::open(store.path("recovery", &older).unwrap())
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(100))
            .unwrap();
        let entries = store.recovery_entries().unwrap();
        assert_eq!(entries[0].id, newer);
        assert_eq!(entries[1].id, older);
        assert!(entries[0].label.contains("[Untitled]"));
        assert!(
            entries
                .iter()
                .all(|e| !e.label.contains("private contents"))
        );
        fs::write(store.path("recovery", &newer).unwrap(), "invalid json").unwrap();
        let entries = store.recovery_entries().unwrap();
        assert!(
            entries
                .iter()
                .find(|e| e.id == newer)
                .unwrap()
                .label
                .contains("Unavailable snapshot")
        );
    }
}
