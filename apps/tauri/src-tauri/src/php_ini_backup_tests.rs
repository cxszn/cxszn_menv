#![cfg(windows)]

use super::*;
use cxszn_menv_core::application::{hash_plan, ChangeAction, ChangeRequest};
use cxszn_menv_core::domain::{
    Activation, Architecture, EnvironmentChangeAction, EnvironmentSnapshot, OperationStatus,
    RuntimeFamily, RuntimeId, RuntimeIniSource, RuntimeRecord, RuntimeSource,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const SOURCE_BYTES: &[u8] = b"; external user configuration\r\nmemory_limit=256M\r\n";

#[derive(Clone)]
struct MemoryStore {
    snapshot: Arc<Mutex<EnvironmentSnapshot>>,
    fail_next_save: Arc<AtomicBool>,
    replace_backup_on_next_save: Arc<Mutex<Option<PathBuf>>>,
}

impl EnvironmentStore for MemoryStore {
    fn load(&self) -> Result<EnvironmentSnapshot, PortError> {
        Ok(self.snapshot.lock().expect("snapshot lock").clone())
    }

    fn save(&self, snapshot: &EnvironmentSnapshot) -> Result<(), PortError> {
        if let Some(path) = self
            .replace_backup_on_next_save
            .lock()
            .expect("replacement hook")
            .take()
        {
            assert!(path.is_file(), "hook runs only after backup creation");
            std::fs::remove_file(&path).expect("replace fixture-owned backup");
            std::fs::write(&path, b"externally replaced backup")
                .expect("create replacement fixture");
            return Err(PortError::Platform("injected save failure".to_owned()));
        }
        if self.fail_next_save.swap(false, Ordering::SeqCst) {
            return Err(PortError::Platform("injected save failure".to_owned()));
        }
        *self.snapshot.lock().expect("snapshot lock") = snapshot.clone();
        Ok(())
    }
}

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    data_root: PathBuf,
    store: MemoryStore,
    service: Orchestrator<MemoryStore>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("menv-ini-backup-{}", Uuid::new_v4()));
        let external_root = root.join("external-php");
        std::fs::create_dir_all(&external_root).expect("create isolated external fixture");
        let source = external_root.join("php.ini");
        std::fs::write(&source, SOURCE_BYTES).expect("create external php.ini fixture");
        let mut runtime = RuntimeRecord::discovered(
            RuntimeId::new("external-php").expect("fixture runtime id"),
            RuntimeFamily::Php,
            "8.5.11",
            Architecture::X64,
            external_root.join("php.exe").to_string_lossy(),
            RuntimeSource::Manual,
        );
        runtime.ini.source = RuntimeIniSource::Explicit;
        runtime.ini.path = Some(source.to_string_lossy().to_string());
        let store = MemoryStore {
            snapshot: Arc::new(Mutex::new(EnvironmentSnapshot {
                runtimes: vec![runtime],
                ..EnvironmentSnapshot::default()
            })),
            fail_next_save: Arc::new(AtomicBool::new(false)),
            replace_backup_on_next_save: Arc::new(Mutex::new(None)),
        };
        Self {
            data_root: root.join("data"),
            root,
            source,
            service: Orchestrator::new(store.clone()),
            store,
        }
    }

    fn plan(&self) -> ChangePlan {
        let mut plan = self
            .service
            .plan_change(ChangeRequest {
                action: ChangeAction::BackupIni,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("external-php").expect("runtime id")),
                target_runtime_id: None,
                idempotency_key: Uuid::new_v4().to_string(),
                actor: "user".to_owned(),
            })
            .expect("backup plan");
        bind_backup_source(&self.service, &mut plan, &self.data_root).expect("bind source");
        plan
    }

    fn assert_rejection_left_no_writes(&self, before: &EnvironmentSnapshot) {
        assert!(
            !self.data_root.join("backups").exists(),
            "rejected requests must not create a backups directory"
        );
        assert_eq!(
            std::fs::read(&self.source).expect("read source"),
            SOURCE_BYTES
        );
        assert_eq!(
            self.service.get_environment_state().expect("state"),
            *before
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let temp_root = std::env::temp_dir();
        assert_eq!(self.root.parent(), Some(temp_root.as_path()));
        assert!(self
            .root
            .file_name()
            .expect("fixture name")
            .to_string_lossy()
            .starts_with("menv-ini-backup-"));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn confirmation(service: &Orchestrator<MemoryStore>, plan: &ChangePlan) -> ConfirmationToken {
    service
        .issue_confirmation_token(plan)
        .expect("explicit test approval")
}

#[test]
fn idempotency_rejects_a_rehashed_different_payload_without_altering_the_backup() {
    let fixture = Fixture::new();
    let mut plan = fixture.plan();
    let first = apply_confirmed_backup(
        &fixture.service,
        &plan,
        Some(&confirmation(&fixture.service, &plan)),
        &fixture.data_root,
    )
    .expect("first");
    plan.backup_source.as_mut().expect("binding").path =
        fixture.root.join("other.ini").to_string_lossy().to_string();
    plan.plan_hash = hash_plan(&plan).expect("rehashed payload");
    let result = apply_confirmed_backup(
        &fixture.service,
        &plan,
        Some(&confirmation(&fixture.service, &plan)),
        &fixture.data_root,
    );
    assert_eq!(result, Err(ApplicationError::IdempotencyConflict));
    assert_eq!(
        std::fs::read(&first.backup_path).expect("backup"),
        SOURCE_BYTES
    );
}

#[test]
fn tampered_completed_backup_is_not_returned_as_a_verified_success() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let first = apply_confirmed_backup(
        &fixture.service,
        &plan,
        Some(&confirmation(&fixture.service, &plan)),
        &fixture.data_root,
    )
    .expect("first");
    std::fs::write(&first.backup_path, b"externally changed backup").expect("changed backup");
    let result = apply_confirmed_backup(
        &fixture.service,
        &plan,
        Some(&confirmation(&fixture.service, &plan)),
        &fixture.data_root,
    );
    assert_eq!(result, Err(ApplicationError::StateChanged));
    assert_eq!(
        std::fs::read(&first.backup_path).expect("preserved external edit"),
        b"externally changed backup"
    );
}

#[test]
fn oversized_source_is_refused_during_planning_without_writing_a_backup() {
    let fixture = Fixture::new();
    std::fs::File::options()
        .write(true)
        .open(&fixture.source)
        .expect("source handle")
        .set_len(MAX_INI_BYTES + 1)
        .expect("oversized source fixture");
    let mut plan = fixture
        .service
        .plan_change(ChangeRequest {
            action: ChangeAction::BackupIni,
            family: RuntimeFamily::Php,
            runtime_id: Some(RuntimeId::new("external-php").expect("id")),
            target_runtime_id: None,
            idempotency_key: "oversized-ini".to_owned(),
            actor: "user".to_owned(),
        })
        .expect("core plan");
    assert!(matches!(
        bind_backup_source(&fixture.service, &mut plan, &fixture.data_root),
        Err(ApplicationError::Port(PortError::InvalidPath(_)))
    ));
    assert!(!fixture.data_root.exists());
}

#[test]
fn legacy_unbound_backup_plan_is_refused_without_filesystem_writes() {
    let fixture = Fixture::new();
    let mut plan = fixture.plan();
    plan.backup_source = None;
    plan.plan_hash = hash_plan(&plan).expect("legacy hash");
    let before = fixture.service.get_environment_state().expect("before");
    assert!(matches!(
        apply_confirmed_backup(
            &fixture.service,
            &plan,
            Some(&confirmation(&fixture.service, &plan)),
            &fixture.data_root
        ),
        Err(ApplicationError::Port(PortError::InvalidPath(_)))
    ));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn source_change_after_confirmation_plan_is_rejected_without_a_backup() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let token = confirmation(&fixture.service, &plan);
    let before = fixture.service.get_environment_state().expect("before");
    std::fs::write(&fixture.source, b"memory_limit=1G\r\n").expect("changed source");
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::StateChanged));
    assert!(!fixture.data_root.join("backups").exists());
    assert_eq!(
        std::fs::read(&fixture.source).expect("source"),
        b"memory_limit=1G\r\n"
    );
    assert_eq!(
        fixture.service.get_environment_state().expect("after"),
        before
    );
}

#[test]
fn completed_retry_preserves_the_original_result_when_runtime_ini_path_changes() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let token = confirmation(&fixture.service, &plan);
    let first = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root)
        .expect("first");
    fixture.store.snapshot.lock().expect("state").runtimes[0]
        .ini
        .path = None;
    let second = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root)
        .expect("idempotent result");
    assert_eq!(second, first);
    assert_eq!(
        std::fs::read(&first.backup_path).expect("backup"),
        SOURCE_BYTES
    );
}

#[test]
fn completed_retry_still_checks_confirmation_and_preserves_the_existing_backup() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let mut token = confirmation(&fixture.service, &plan);
    let first = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root)
        .expect("first");
    token.plan_hash = "not-the-approved-hash".to_owned();
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::ConfirmationInvalid));
    assert_eq!(
        std::fs::read(&first.backup_path).expect("backup"),
        SOURCE_BYTES
    );
    assert_eq!(
        fixture
            .service
            .get_environment_state()
            .expect("state")
            .operations
            .len(),
        1
    );
}

#[test]
fn missing_confirmation_creates_no_backup_directory_or_operation() {
    let fixture = Fixture::new();
    let before = fixture.service.get_environment_state().expect("before");
    let result =
        apply_confirmed_backup(&fixture.service, &fixture.plan(), None, &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::ConfirmationRequired));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn never_issued_confirmation_cannot_create_backup_or_operation() {
    let fixture = Fixture::new();
    let before = fixture.service.get_environment_state().expect("before");
    let plan = fixture.plan();
    let token = ConfirmationToken {
        value: "never-issued-token".to_owned(),
        plan_id: plan.plan_id.clone(),
        plan_hash: plan.plan_hash.clone(),
        actor: plan.request.actor.clone(),
        expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
    };
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::ConfirmationInvalid));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn confirmation_flag_cannot_bypass_backup_confirmation_or_create_directory() {
    let fixture = Fixture::new();
    let before = fixture.service.get_environment_state().expect("before");
    let mut plan = fixture.plan();
    plan.requires_confirmation = false;
    plan.plan_hash = hash_plan(&plan).expect("hash plan with lowered confirmation flag");
    let result = apply_confirmed_backup(&fixture.service, &plan, None, &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::ConfirmationRequired));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn invalid_confirmation_creates_no_backup_directory_or_operation() {
    let fixture = Fixture::new();
    let before = fixture.service.get_environment_state().expect("before");
    let plan = fixture.plan();
    let mut token = confirmation(&fixture.service, &plan);
    token.actor = "another-user".to_owned();
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::ConfirmationInvalid));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn expired_plan_creates_no_backup_directory_or_operation() {
    let fixture = Fixture::new();
    let before = fixture.service.get_environment_state().expect("before");
    let mut plan = fixture.plan();
    let mut token = confirmation(&fixture.service, &plan);
    plan.expires_at_epoch_seconds = plan.created_at_epoch_seconds;
    plan.plan_hash = hash_plan(&plan).expect("hash expired plan");
    token.expires_at_epoch_seconds = plan.expires_at_epoch_seconds;
    token.plan_hash = plan.plan_hash.clone();
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::PlanExpired));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn tampered_plan_creates_no_backup_directory_or_operation() {
    let fixture = Fixture::new();
    let before = fixture.service.get_environment_state().expect("before");
    let mut plan = fixture.plan();
    let token = confirmation(&fixture.service, &plan);
    plan.request.actor = "changed-after-planning".to_owned();
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::PlanTampered));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn changed_state_creates_no_backup_directory_or_operation() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let token = confirmation(&fixture.service, &plan);
    fixture
        .store
        .snapshot
        .lock()
        .expect("snapshot lock")
        .updated_at = "changed-after-planning".to_owned();
    let before = fixture
        .service
        .get_environment_state()
        .expect("changed state");
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert_eq!(result, Err(ApplicationError::StateChanged));
    fixture.assert_rejection_left_no_writes(&before);
}

#[test]
fn confirmed_backup_preserves_source_activation_and_records_one_operation() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let result = apply_confirmed_backup(
        &fixture.service,
        &plan,
        Some(&confirmation(&fixture.service, &plan)),
        &fixture.data_root,
    )
    .expect("confirmed backup succeeds");
    assert_eq!(result.runtime_id, "external-php");
    assert_eq!(Path::new(&result.source_path), fixture.source);
    assert!(Path::new(&result.backup_path).starts_with(fixture.data_root.join("backups")));
    assert_eq!(
        std::fs::read(&result.backup_path).expect("read backup"),
        SOURCE_BYTES
    );
    assert_eq!(
        std::fs::read(&fixture.source).expect("read source"),
        SOURCE_BYTES
    );
    let state = fixture.service.get_environment_state().expect("state");
    assert_eq!(state.runtimes[0].activation, Activation::Disabled);
    assert!(!state.path_lease.enabled);
    assert_eq!(state.operations.len(), 1);
    assert_eq!(
        state.operations[0].action,
        EnvironmentChangeAction::BackupIni
    );
    assert_eq!(state.operations[0].status, OperationStatus::Succeeded);
}

#[test]
fn repeated_confirmed_backup_returns_same_backup_and_one_operation() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let token = confirmation(&fixture.service, &plan);
    let first = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root)
        .expect("first backup succeeds");
    let second = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root)
        .expect("idempotent backup retry succeeds");
    assert_eq!(first, second);
    assert_eq!(
        std::fs::read(&first.backup_path).expect("backup"),
        SOURCE_BYTES
    );
    assert_eq!(
        fixture
            .service
            .get_environment_state()
            .expect("state")
            .operations
            .len(),
        1
    );
    assert_eq!(
        std::fs::read_dir(fixture.data_root.join("backups").join("php-ini"))
            .expect("backup directory")
            .count(),
        1
    );
}

#[test]
fn failed_state_save_preserves_orphan_source_and_existing_backup_without_success() {
    let fixture = Fixture::new();
    let backup_dir = fixture.data_root.join("backups").join("php-ini");
    std::fs::create_dir_all(&backup_dir).expect("existing backup directory");
    let existing = backup_dir.join("prior.php.ini.bak");
    std::fs::write(&existing, b"previous backup").expect("existing backup fixture");
    let before = fixture.service.get_environment_state().expect("before");
    let plan = fixture.plan();
    fixture.store.fail_next_save.store(true, Ordering::SeqCst);
    let result = apply_confirmed_backup(
        &fixture.service,
        &plan,
        Some(&confirmation(&fixture.service, &plan)),
        &fixture.data_root,
    );
    let orphan_path = backup_dir.join(format!("{}.php.ini.bak", plan.plan_id));
    assert!(matches!(
        &result,
        Err(ApplicationError::Port(PortError::BackupRecoveryRequired { backup_path }))
            if Path::new(backup_path) == orphan_path
    ));
    assert_eq!(
        std::fs::read(&existing).expect("existing backup"),
        b"previous backup"
    );
    assert_eq!(
        std::fs::read(&fixture.source).expect("source"),
        SOURCE_BYTES
    );
    assert_eq!(
        std::fs::read(&orphan_path).expect("orphan backup remains available for recovery"),
        SOURCE_BYTES
    );
    assert_eq!(std::fs::read_dir(&backup_dir).expect("backups").count(), 2);
    assert_eq!(
        fixture.service.get_environment_state().expect("state"),
        before
    );
}

#[test]
fn failed_state_save_preserves_replacement_backup_and_prior_operation() {
    let fixture = Fixture::new();
    let first_plan = fixture.plan();
    let existing = apply_confirmed_backup(
        &fixture.service,
        &first_plan,
        Some(&confirmation(&fixture.service, &first_plan)),
        &fixture.data_root,
    )
    .expect("existing completed backup");
    let before = fixture.service.get_environment_state().expect("before");
    let plan = fixture.plan();
    let replacement_path = fixture
        .data_root
        .join("backups")
        .join("php-ini")
        .join(format!("{}.php.ini.bak", plan.plan_id));
    assert!(replacement_path.starts_with(&fixture.data_root));
    *fixture
        .store
        .replace_backup_on_next_save
        .lock()
        .expect("replacement hook") = Some(replacement_path.clone());

    let result = apply_confirmed_backup(
        &fixture.service,
        &plan,
        Some(&confirmation(&fixture.service, &plan)),
        &fixture.data_root,
    );
    assert!(matches!(
        &result,
        Err(ApplicationError::Port(PortError::BackupRecoveryRequired { backup_path }))
            if Path::new(backup_path) == replacement_path
    ));
    assert_eq!(
        std::fs::read(&replacement_path).expect("external replacement must not be deleted"),
        b"externally replaced backup"
    );
    assert_eq!(
        std::fs::read(&existing.backup_path).expect("prior backup"),
        SOURCE_BYTES
    );
    assert_eq!(
        std::fs::read(&fixture.source).expect("source"),
        SOURCE_BYTES
    );
    assert_eq!(
        fixture.service.get_environment_state().expect("state"),
        before
    );
}

#[test]
fn plan_id_path_traversal_is_rejected_before_backup_directory_creation() {
    let fixture = Fixture::new();
    let before = fixture.service.get_environment_state().expect("before");
    let mut plan = fixture.plan();
    let mut token = confirmation(&fixture.service, &plan);
    // The escape remains inside this unique fixture even with the buggy adapter.
    plan.plan_id = "../../escaped".to_owned();
    plan.plan_hash = hash_plan(&plan).expect("hash traversal plan");
    token.plan_id = plan.plan_id.clone();
    token.plan_hash = plan.plan_hash.clone();
    let result = apply_confirmed_backup(&fixture.service, &plan, Some(&token), &fixture.data_root);
    assert!(matches!(
        result,
        Err(ApplicationError::Port(PortError::InvalidPath(_)))
    ));
    fixture.assert_rejection_left_no_writes(&before);
    assert!(!fixture.data_root.join("escaped.php.ini.bak").exists());
}
