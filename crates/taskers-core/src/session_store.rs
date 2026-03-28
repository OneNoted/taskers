use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use taskers_domain::{AppModel, PersistedSession, SESSION_SCHEMA_VERSION};
use taskers_paths::default_session_path as shared_default_session_path;

pub fn default_session_path() -> PathBuf {
    shared_default_session_path()
}

pub fn load_or_bootstrap(path: &Path, demo: bool) -> Result<AppModel> {
    if path.exists() {
        match load_session(path) {
            Ok(mut model) => {
                model.recover_interrupted_agent_resumes();
                Ok(model)
            }
            Err(error) => {
                backup_incompatible_session(path)?;
                eprintln!("failed to load session from {}: {}", path.display(), error);
                if demo {
                    Ok(AppModel::demo())
                } else {
                    Ok(AppModel::new("Workspace 1"))
                }
            }
        }
    } else if demo {
        Ok(AppModel::demo())
    } else {
        Ok(AppModel::new("Workspace 1"))
    }
}

pub fn load_session(path: &Path) -> Result<AppModel> {
    let data = fs::read_to_string(path)?;
    let session: PersistedSession = serde_json::from_str(&data)?;

    if session.schema_version != SESSION_SCHEMA_VERSION {
        bail!(
            "unsupported session schema version {}, expected {}",
            session.schema_version,
            SESSION_SCHEMA_VERSION
        );
    }

    Ok(session.model)
}

pub fn save_session(path: &Path, model: &AppModel) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let session = model.snapshot();
    let data = serde_json::to_string_pretty(&session)?;
    fs::write(path, data)?;

    Ok(())
}

fn backup_incompatible_session(path: &Path) -> Result<()> {
    let backup_path = path.with_extension("json.bak");
    if backup_path.exists() {
        fs::remove_file(&backup_path)?;
    }
    fs::rename(path, backup_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{load_session, save_session};
    use taskers_domain::AppModel;

    #[test]
    fn roundtrips_session_files() {
        let tempdir = tempdir().expect("tempdir");
        let session_path = tempdir.path().join("session.json");
        let model = AppModel::demo();

        save_session(&session_path, &model).expect("session saved");
        let loaded = load_session(&session_path).expect("session loaded");

        assert_eq!(loaded, model);
    }
}
