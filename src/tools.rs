//! File capabilities are prepared without reading content, then used only after approval.
use anyhow::{Context, Result, bail};
use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_CONTEXT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub workspace: PathBuf,
    pub path: PathBuf,
    pub operation: String,
}

#[derive(Default)]
pub struct Permissions {
    pub session: Vec<Grant>,
    pub saved: Vec<Grant>,
}
impl Permissions {
    pub fn allows(&self, target: &Target) -> bool {
        let grant = target.grant();
        self.session.contains(&grant) || self.saved.contains(&grant)
    }
    pub fn clear_session(&mut self) {
        self.session.clear();
    }
}

#[derive(Debug)]
pub struct Target {
    pub workspace: PathBuf,
    pub path: PathBuf,
    pub operation: String,
    dir: Arc<Dir>,
    filename: Option<std::ffi::OsString>,
    metadata: fs::Metadata,
}
impl Target {
    pub fn grant(&self) -> Grant {
        Grant {
            workspace: self.workspace.clone(),
            path: self.path.clone(),
            operation: self.operation.clone(),
        }
    }
    pub fn outside_workspace(&self) -> bool {
        !self.path.starts_with(&self.workspace)
    }
}

pub fn sensitive(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".ssh"
                | ".gnupg"
                | ".aws"
                | ".azure"
                | ".kube"
                | ".git"
                | "credentials"
                | "credentials.json"
        ) || name == ".env"
            || name.starts_with(".env.")
            || [".pem", ".key", ".p12", ".pfx"]
                .iter()
                .any(|ext| name.ends_with(ext))
    })
}

pub fn prepare(workspace: &Path, requested: &str, operation: &str) -> Result<Target> {
    if !matches!(operation, "read_file" | "list_directory") {
        bail!("Unknown file operation");
    }
    let workspace = workspace.canonicalize()?;
    let requested = Path::new(requested);
    let joined = if requested.is_absolute() {
        requested.to_owned()
    } else {
        workspace.join(requested)
    };
    if sensitive(&joined) {
        bail!("Sensitive paths are excluded from agent access");
    }
    let path = joined.canonicalize().context("Tool path does not exist")?;
    if sensitive(&path) {
        bail!("Sensitive symlink target is excluded from agent access");
    }
    let metadata = fs::metadata(&path)?;
    let (dir, filename) = if operation == "read_file" {
        if !metadata.is_file() {
            bail!("Not a regular file");
        }
        if metadata.len() > MAX_CONTEXT_BYTES as u64 {
            bail!("Agent reads are limited to 256 KiB");
        }
        let parent = path.parent().context("Missing parent")?;
        (
            Dir::open_ambient_dir(parent, ambient_authority())?,
            Some(path.file_name().context("Missing filename")?.to_owned()),
        )
    } else {
        if !metadata.is_dir() {
            bail!("Not a directory");
        }
        (Dir::open_ambient_dir(&path, ambient_authority())?, None)
    };
    Ok(Target {
        workspace,
        path,
        operation: operation.into(),
        dir: Arc::new(dir),
        filename,
        metadata,
    })
}

/// Consume the prepared capability after explicit authorization.
pub fn execute(target: Target) -> Result<String> {
    if target.path.canonicalize()? != target.path || sensitive(&target.path) {
        bail!("Tool path changed after permission was requested");
    }
    if let Some(name) = target.filename {
        // cap-std prevents final symlink traversal outside the anchored parent directory.
        let mut file = target.dir.open(name)?.into_std();
        let now = file.metadata()?;
        if !same_file(&target.metadata, &now) || !now.is_file() {
            bail!("File identity changed; request access again");
        }
        let mut bytes = vec![];
        file.by_ref()
            .take(MAX_CONTEXT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CONTEXT_BYTES || bytes.contains(&0) {
            bail!("File is binary or exceeds the agent size limit");
        }
        String::from_utf8(bytes).context("Agent reads require UTF-8")
    } else {
        let mut entries = vec![];
        for entry in target.dir.entries()? {
            let entry = entry?;
            if sensitive(&target.path.join(entry.file_name())) {
                continue;
            }
            if entries.len() >= 1000 {
                bail!("Directory listing exceeds 1000 entries");
            }
            let kind = entry.file_type()?;
            entries.push(format!(
                "{}{}",
                entry.file_name().to_string_lossy(),
                if kind.is_dir() {
                    "/"
                } else if kind.is_symlink() {
                    " [symlink]"
                } else {
                    ""
                }
            ));
        }
        entries.sort();
        let output = entries.join("\n");
        if output.len() > MAX_CONTEXT_BYTES {
            bail!("Directory listing exceeds size limit");
        }
        Ok(output)
    }
}

fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        a.len() == b.len() && a.modified().ok() == b.modified().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_implicit_workspace_grants() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("note"), "text").unwrap();
        let target = prepare(dir.path(), "note", "read_file").unwrap();
        let mut p = Permissions::default();
        assert!(!p.allows(&target));
        p.session.push(target.grant());
        assert!(p.allows(&target));
        p.clear_session();
        assert!(!p.allows(&target));
    }
    #[test]
    fn sensitive_paths_are_denied() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".env"), "SECRET").unwrap();
        assert!(prepare(dir.path(), ".env", "read_file").is_err());
        let target = prepare(dir.path(), ".", "list_directory").unwrap();
        assert!(!execute(target).unwrap().contains(".env"));
    }
    #[cfg(unix)]
    #[test]
    fn symlink_escape_requires_distinct_grant_and_races_fail() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("note"), "external").unwrap();
        std::os::unix::fs::symlink(outside.path().join("note"), dir.path().join("link")).unwrap();
        let target = prepare(dir.path(), "link", "read_file").unwrap();
        assert!(target.outside_workspace());
        fs::write(dir.path().join("original"), "safe").unwrap();
        let target = prepare(dir.path(), "original", "read_file").unwrap();
        fs::remove_file(dir.path().join("original")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("note"), dir.path().join("original"))
            .unwrap();
        assert!(execute(target).is_err());
    }
}
