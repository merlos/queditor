//! Credential references and private secret storage. Secrets never enter config.yaml.
use crate::config::{Credential, config_dir};
use anyhow::{Context, Result, bail};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

const MAX_CREDENTIAL_BYTES: u64 = 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CredentialFile {
    schema_version: u32,
    credentials: BTreeMap<String, String>,
}

pub fn resolve(reference: &Credential) -> Result<SecretString> {
    let value = if let Some(name) = &reference.env {
        std::env::var(name)
            .with_context(|| format!("Credential environment variable {name} is not set"))?
    } else if let Some(name) = &reference.file {
        read_file()?
            .credentials
            .remove(name)
            .context("Credential was not found in credentials.yaml")?
    } else {
        bail!("No credential source configured");
    };
    if value.is_empty() {
        bail!("Credential is empty");
    }
    Ok(SecretString::from(value))
}

pub fn save_file(name: &str, secret: &SecretString) -> Result<()> {
    validate_name(name)?;
    let mut values = read_file()?;
    values.schema_version = 1;
    values
        .credentials
        .insert(name.into(), secret.expose_secret().into());
    write_file(&values)
}

pub fn delete(reference: &Credential) -> Result<()> {
    if let Some(name) = &reference.file {
        let mut values = read_file()?;
        values.credentials.remove(name);
        write_file(&values)?;
    }
    Ok(())
}

fn credential_path() -> std::path::PathBuf {
    config_dir().join("credentials.yaml")
}

fn read_file() -> Result<CredentialFile> {
    read_file_at(&credential_path())
}

fn read_file_at(path: &Path) -> Result<CredentialFile> {
    if !path.exists() {
        return Ok(CredentialFile {
            schema_version: 1,
            credentials: BTreeMap::new(),
        });
    }
    reject_symlink(path)?;
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_CREDENTIAL_BYTES {
        bail!("credentials.yaml exceeds size limit");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("credentials.yaml must not be accessible by group or other users");
        }
    }
    let values: CredentialFile = serde_yaml_ng::from_str(&fs::read_to_string(path)?)?;
    if values.schema_version != 1 {
        bail!("Unsupported credential file version");
    }
    Ok(values)
}

fn write_file(values: &CredentialFile) -> Result<()> {
    let directory = config_dir();
    fs::create_dir_all(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    }
    let path = credential_path();
    if path.exists() {
        reject_symlink(&path)?;
    }
    let bytes = serde_yaml_ng::to_string(values)?;
    if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        bail!("credentials.yaml exceeds size limit");
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temporary.write_all(bytes.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("Credential symlinks are not supported");
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        bail!("Invalid credential name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_names_are_restricted() {
        assert!(validate_name("company-prod").is_ok());
        assert!(validate_name("../secret").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn credential_file_rejects_permissive_permissions_and_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credentials.yaml");
        fs::write(&path, "schema_version: 1\ncredentials: {}\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_file_at(&path).is_err());

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_file_at(&path).is_ok());

        let link = directory.path().join("credentials-link.yaml");
        symlink(&path, &link).unwrap();
        assert!(read_file_at(&link).is_err());
    }
}
