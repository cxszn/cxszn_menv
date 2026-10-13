use super::*;
use cxszn_menv_ports::{IniBackupArtifact, PhpIniBackupStore};

pub const MAX_INI_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhpIniBackupInfo {
    pub runtime_id: String,
    pub source_path: String,
    pub backup_path: String,
}

impl<S: EnvironmentStore, P: UserPathStore> Orchestrator<S, P> {
    /// Bind the bytes the user will approve. This is a read-only operation.
    pub fn bind_php_ini_backup_source(
        &self,
        plan: &mut ChangePlan,
        backup_store: &dyn PhpIniBackupStore,
    ) -> Result<(), ApplicationError> {
        let runtime_id = backup_runtime_id(plan)?;
        let snapshot = self.store.load()?;
        let source = snapshot
            .runtimes
            .iter()
            .find(|runtime| &runtime.id == runtime_id)
            .ok_or(ApplicationError::RuntimeNotFound)?
            .ini
            .path
            .as_deref()
            .ok_or_else(|| {
                PortError::InvalidPath("php.ini was not found for this runtime".into())
            })?;
        let contents = backup_store.read_source(Path::new(source))?;
        check_ini_size(&contents)?;
        plan.backup_source = Some(BackupSourceBinding {
            path: source.to_owned(),
            sha256: hash_bytes(&contents),
        });
        plan.plan_hash = hash_plan(plan)?;
        Ok(())
    }

    /// Callers hold the native operation lock through file and state effects.
    /// A created artifact is retained on failure: a path alone cannot prove
    /// ownership well enough for destructive rollback.
    pub fn apply_php_ini_backup(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
        backup_store: &dyn PhpIniBackupStore,
    ) -> Result<PhpIniBackupInfo, ApplicationError> {
        Uuid::parse_str(&plan.plan_id)
            .map_err(|_| PortError::InvalidPath("backup plan id must be a UUID".into()))?;
        let (mut snapshot, existing) = self.prepare_change(plan, confirmation_token)?;
        let runtime_id = backup_runtime_id(plan)?;
        let binding = plan.backup_source.as_ref().ok_or_else(|| {
            PortError::InvalidPath("backup plan must bind its source; create a new plan".into())
        })?;
        if binding.sha256.len() != 64 || !binding.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(PortError::InvalidPath("backup plan SHA-256 is invalid".into()).into());
        }
        let artifact = if let Some(receipt) = existing {
            if receipt.status != OperationStatus::Succeeded {
                return Err(PortError::Platform(
                    "previous backup operation did not succeed".into(),
                )
                .into());
            }
            let artifact = backup_store.get_backup(&plan.plan_id)?;
            verify_artifact(&artifact, binding)?;
            artifact
        } else {
            let runtime = snapshot
                .runtimes
                .iter()
                .find(|r| &r.id == runtime_id)
                .ok_or(ApplicationError::RuntimeNotFound)?;
            if runtime.ini.path.as_deref().map(normalize_runtime_path)
                != Some(normalize_runtime_path(&binding.path))
            {
                return Err(ApplicationError::StateChanged);
            }
            let contents = backup_store.read_source(Path::new(&binding.path))?;
            check_ini_size(&contents)?;
            if !hash_bytes(&contents).eq_ignore_ascii_case(&binding.sha256) {
                return Err(ApplicationError::StateChanged);
            }
            update_snapshot_after_change(
                &mut snapshot,
                &plan.request,
                Uuid::new_v4().to_string(),
                EnvironmentChangeAction::BackupIni,
                plan.plan_id.clone(),
                unix_epoch_iso(now_epoch_seconds()),
                plan.plan_hash.clone(),
            )?;
            let artifact = backup_store.create_backup(&plan.plan_id, &contents)?;
            if verify_artifact(&artifact, binding).is_err() || self.store.save(&snapshot).is_err() {
                return Err(PortError::BackupRecoveryRequired {
                    backup_path: artifact.path.to_string_lossy().into_owned(),
                }
                .into());
            }
            artifact
        };
        Ok(PhpIniBackupInfo {
            runtime_id: runtime_id.as_str().to_owned(),
            source_path: binding.path.clone(),
            backup_path: artifact.path.to_string_lossy().into_owned(),
        })
    }
}

fn backup_runtime_id(plan: &ChangePlan) -> Result<&RuntimeId, ApplicationError> {
    if plan.request.action != ChangeAction::BackupIni
        || plan.request.target_runtime_id.is_some()
        || plan.request.family != RuntimeFamily::Php
    {
        return Err(ApplicationError::UnsupportedChangeAction);
    }
    plan.request
        .runtime_id
        .as_ref()
        .ok_or(ApplicationError::MissingRuntimeId)
}

fn check_ini_size(contents: &[u8]) -> Result<(), ApplicationError> {
    if contents.len() > MAX_INI_BYTES {
        return Err(PortError::InvalidPath("php.ini exceeds the 4 MiB backup limit".into()).into());
    }
    Ok(())
}

fn verify_artifact(
    artifact: &IniBackupArtifact,
    binding: &BackupSourceBinding,
) -> Result<(), ApplicationError> {
    if !artifact.sha256.eq_ignore_ascii_case(&binding.sha256) {
        return Err(ApplicationError::StateChanged);
    }
    Ok(())
}
