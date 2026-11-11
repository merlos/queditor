//! File and web capabilities are prepared without fetching content, then used after approval.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}
impl Grant {
    pub fn location(&self) -> String {
        self.url
            .clone()
            .unwrap_or_else(|| self.path.display().to_string())
    }
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
    capability: Capability,
}

#[derive(Debug)]
enum Capability {
    File(FileTarget),
    Web { workspace: PathBuf, url: url::Url },
}

#[derive(Debug)]
struct FileTarget {
    pub workspace: PathBuf,
    pub path: PathBuf,
    pub operation: String,
    dir: Arc<Dir>,
    filename: Option<std::ffi::OsString>,
    metadata: fs::Metadata,
}
impl Target {
    pub fn grant(&self) -> Grant {
        match &self.capability {
            Capability::File(file) => Grant {
                workspace: file.workspace.clone(),
                path: file.path.clone(),
                operation: file.operation.clone(),
                url: None,
            },
            Capability::Web { workspace, url } => Grant {
                workspace: workspace.clone(),
                path: PathBuf::new(),
                operation: "webfetch".into(),
                url: Some(url.to_string()),
            },
        }
    }
    pub fn operation(&self) -> &str {
        match &self.capability {
            Capability::File(file) => &file.operation,
            Capability::Web { .. } => "webfetch",
        }
    }
    pub fn location(&self) -> String {
        self.grant().location()
    }
    pub fn is_web(&self) -> bool {
        matches!(self.capability, Capability::Web { .. })
    }
    pub fn outside_workspace(&self) -> bool {
        match &self.capability {
            Capability::File(file) => !file.path.starts_with(&file.workspace),
            Capability::Web { .. } => false,
        }
    }
}

/// URL validation is local: DNS resolution and HTTP wait until approval.
pub fn prepare_web(workspace: &Path, requested: &str) -> Result<Target> {
    Ok(Target {
        capability: Capability::Web {
            workspace: workspace.canonicalize()?,
            url: crate::web::validate_url(requested)?,
        },
    })
}

pub async fn execute_approved(target: Target) -> Result<String> {
    match target.capability {
        Capability::Web { url, .. } => crate::web::fetch(url).await,
        Capability::File(file) => tokio::task::spawn_blocking(move || execute_file(file)).await?,
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
        capability: Capability::File(FileTarget {
            workspace,
            path,
            operation: operation.into(),
            dir: Arc::new(dir),
            filename,
            metadata,
        }),
    })
}

/// Consume the prepared capability after explicit authorization.
pub fn execute(target: Target) -> Result<String> {
    match target.capability {
        Capability::File(file) => execute_file(file),
        Capability::Web { .. } => bail!("Web access requires asynchronous execution"),
    }
}

fn execute_file(target: FileTarget) -> Result<String> {
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
    fn web_grants_are_exact_urls_operations_and_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let target = prepare_web(dir.path(), "https://example.com/page?q=1").unwrap();
        let mut permissions = Permissions::default();
        assert!(!permissions.allows(&target));
        assert_eq!(target.location(), "https://example.com/page?q=1");
        assert_eq!(target.operation(), "webfetch");
        permissions.session.push(target.grant());
        assert!(permissions.allows(&target));
        for url in [
            "https://example.com/page?q=2",
            "https://example.com/other",
            "http://example.com/page?q=1",
        ] {
            assert!(!permissions.allows(&prepare_web(dir.path(), url).unwrap()));
        }
        assert!(
            !permissions
                .allows(&prepare_web(other.path(), "https://example.com/page?q=1").unwrap())
        );
        permissions.clear_session();
        assert!(!permissions.allows(&target));
        permissions.saved.push(target.grant());
        assert!(permissions.allows(&target));
    }

    #[test]
    fn old_file_grants_remain_compatible_and_web_grants_round_trip() {
        let grant: Grant = serde_json::from_str(
            r#"{"workspace":"/project","path":"/project/note","operation":"read_file"}"#,
        )
        .unwrap();
        assert!(grant.url.is_none());
        let dir = tempfile::tempdir().unwrap();
        let web = prepare_web(dir.path(), "https://example.com/")
            .unwrap()
            .grant();
        let restored: Grant = serde_json::from_str(&serde_json::to_string(&web).unwrap()).unwrap();
        assert_eq!(web, restored);
    }
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
