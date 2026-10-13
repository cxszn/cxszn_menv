use super::*;
use cxszn_menv_core::application::Orchestrator;
use cxszn_menv_core::domain::{RuntimeFamily, RuntimeId};
use cxszn_menv_core::ports::EmptyEnvironmentStore;

fn native_plan(action: ChangeAction, runtime_id: Option<RuntimeId>) -> ChangePlan {
    let now = now_epoch_seconds();
    ChangePlan {
        plan_id: Uuid::new_v4().to_string(),
        plan_hash: String::new(),
        request: ChangeRequest {
            action,
            family: RuntimeFamily::Php,
            runtime_id,
            target_runtime_id: None,
            idempotency_key: Uuid::new_v4().to_string(),
            actor: "user".to_owned(),
        },
        requires_confirmation: true,
        created_at_epoch_seconds: now,
        expires_at_epoch_seconds: now + 300,
        snapshot_hash: snapshot_hash(&EnvironmentSnapshot::default()).expect("snapshot hash"),
        backup_source: None,
    }
}

fn never_issued_confirmation(plan: &ChangePlan) -> ConfirmationToken {
    ConfirmationToken {
        value: "never-issued-token".to_owned(),
        plan_id: plan.plan_id.clone(),
        plan_hash: plan.plan_hash.clone(),
        actor: plan.request.actor.clone(),
        expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
    }
}

#[test]
fn unissued_confirmation_cannot_authorize_php_artifact_installation() {
    let mut plan = native_plan(ChangeAction::Install, None);
    let spec = cxszn_menv_core::windows::install::PhpArtifactInstallSpec {
        url: "https://downloads.php.net/~windows/releases/php-8.5.11-nts-Win32-vs17-x64.zip"
            .to_owned(),
        file_name: "php-8.5.11-nts-Win32-vs17-x64.zip".to_owned(),
        expected_size_bytes: 10,
        expected_sha256: "a".repeat(64),
        staging_root: PathBuf::from(r"C:\isolated-confirmation-fixture\staging"),
        managed_directory: PathBuf::from(r"C:\isolated-confirmation-fixture\versions\php"),
        entrypoint: "php.exe".to_owned(),
        journal_path: None,
    };
    plan.plan_hash = install_plan_hash(&plan, &spec).expect("artifact-bound plan hash");
    let token = never_issued_confirmation(&plan);
    let service = Orchestrator::new(EmptyEnvironmentStore);

    assert_eq!(
        validate_install_confirmation(&plan, Some(&token), &service),
        Err(ApplicationError::ConfirmationInvalid)
    );
    let issued = service
        .issue_confirmation_token(&plan)
        .expect("explicit approval");
    assert_eq!(
        validate_install_confirmation(&plan, Some(&issued), &service),
        Ok(())
    );
}

#[test]
fn unissued_confirmation_cannot_authorize_manual_php_registration() {
    let runtime = registration_fixture();
    let mut plan = native_plan(ChangeAction::Register, Some(runtime.id.clone()));
    plan.plan_hash = registration_plan_hash(&plan, &runtime).expect("plan hash");
    let token = never_issued_confirmation(&plan);
    let service = Orchestrator::new(EmptyEnvironmentStore);

    assert_eq!(
        validate_php_registration_confirmation(&plan, Some(&token), &service),
        Err(ApplicationError::ConfirmationInvalid)
    );
    let issued = service
        .issue_confirmation_token(&plan)
        .expect("explicit approval");
    assert_eq!(
        validate_php_registration_confirmation(&plan, Some(&issued), &service),
        Ok(())
    );
}

fn registration_fixture() -> cxszn_menv_core::domain::RuntimeRecord {
    use cxszn_menv_core::domain::{Architecture, RuntimeRecord, RuntimeSource};
    RuntimeRecord::discovered(
        RuntimeId::new("php:win:c:\\fixture\\php.exe").expect("runtime id"),
        RuntimeFamily::Php,
        "8.5.11",
        Architecture::X64,
        r"C:\fixture\php.exe",
        RuntimeSource::Manual,
    )
}

#[test]
fn cli_confirmation_rejects_displayed_action_that_does_not_match_digest() {
    let mut plan = native_plan(ChangeAction::Enable, Some(registration_fixture().id));
    plan.plan_hash = cxszn_menv_core::application::hash_plan(&plan).expect("plan hash");
    plan.request.action = ChangeAction::Disable;
    assert_eq!(
        validate_native_plan_payload(&plan, None),
        Err(ApplicationError::PlanTampered)
    );
}

#[test]
fn registration_digest_binds_the_entire_verified_runtime_payload() {
    let runtime = registration_fixture();
    let plan = native_plan(ChangeAction::Register, Some(runtime.id.clone()));
    let approved_hash = registration_plan_hash(&plan, &runtime).expect("registration hash");
    let mut replaced = runtime;
    replaced.executable_path = r"C:\different\php.exe".to_owned();
    assert_ne!(
        registration_plan_hash(&plan, &replaced).expect("replaced hash"),
        approved_hash
    );
}

#[test]
fn cli_registration_rejects_replaced_payload_and_legacy_unbound_plan() {
    let runtime = registration_fixture();
    let mut plan = native_plan(ChangeAction::Register, Some(runtime.id.clone()));
    plan.plan_hash = registration_plan_hash(&plan, &runtime).expect("registration hash");
    assert_eq!(validate_native_plan_payload(&plan, Some(&runtime)), Ok(()));
    let mut replaced = runtime.clone();
    replaced.executable_path = r"C:\different\php.exe".to_owned();
    assert_eq!(
        validate_native_plan_payload(&plan, Some(&replaced)),
        Err(ApplicationError::PlanTampered)
    );
    plan.plan_hash = cxszn_menv_core::application::hash_plan(&plan).expect("legacy hash");
    assert_eq!(
        validate_native_plan_payload(&plan, Some(&runtime)),
        Err(ApplicationError::PlanTampered)
    );
}

#[test]
fn registration_rechecks_confirmation_expiry_after_wait_before_the_effect() {
    let runtime = registration_fixture();
    let mut plan = native_plan(ChangeAction::Register, Some(runtime.id.clone()));
    plan.expires_at_epoch_seconds = now_epoch_seconds() + 1;
    plan.plan_hash = registration_plan_hash(&plan, &runtime).expect("registration hash");
    let service = Orchestrator::new(EmptyEnvironmentStore);
    let token = service
        .issue_confirmation_token(&plan)
        .expect("issued before wait");
    service
        .validate_confirmation_token(&plan, Some(&token))
        .expect("initial validation");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        apply_confirmed_php_registration(&service, &plan, Some(&token), runtime),
        Err(ApplicationError::PlanExpired)
    );
    assert!(service
        .get_environment_state()
        .expect("no writes")
        .runtimes
        .is_empty());
}
