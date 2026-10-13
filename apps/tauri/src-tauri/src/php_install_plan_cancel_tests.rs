use super::*;
use cxszn_menv_core::application::Orchestrator;
use cxszn_menv_core::ports::EmptyEnvironmentStore;
use cxszn_menv_core::windows::SqliteEnvironmentStore;

fn fixture() -> (SqliteEnvironmentStore, PhpInstallPlanRecord) {
    let root = std::env::temp_dir().join(format!("menv-install-cancel-{}", Uuid::new_v4()));
    let store = SqliteEnvironmentStore::new(root.join("state.sqlite3"), None);
    let catalog = cxszn_menv_core::windows::embedded_php_catalog().unwrap();
    let spec =
        cxszn_menv_core::windows::create_php_install_spec(&catalog, "php-8.5.11-nts-x64", &root)
            .unwrap();
    let mut plan = ChangePlan {
        plan_id: Uuid::new_v4().to_string(),
        plan_hash: String::new(),
        request: ChangeRequest {
            action: ChangeAction::Install,
            family: cxszn_menv_core::domain::RuntimeFamily::Php,
            runtime_id: None,
            target_runtime_id: None,
            idempotency_key: Uuid::new_v4().to_string(),
            actor: "test".to_owned(),
        },
        requires_confirmation: true,
        created_at_epoch_seconds: now_epoch_seconds(),
        expires_at_epoch_seconds: now_epoch_seconds() + 300,
        snapshot_hash: snapshot_hash(&EnvironmentSnapshot::default()).unwrap(),
        backup_source: None,
    };
    plan.plan_hash = install_plan_hash(&plan, &spec).unwrap();
    (
        store,
        PhpInstallPlanRecord {
            plan,
            spec,
            result: None,
            status: PhpInstallPlanStatus::Pending,
        },
    )
}

#[test]
fn pending_install_cancellation_is_durable_idempotent_and_cannot_be_reapplied() {
    let (store, record) = fixture();
    let stale = HashMap::from([(record.plan.plan_id.clone(), record.clone())]);
    persist_php_install_plans_in_store(&store, &stale).unwrap();
    cancel_php_install_plan_in_store(&store, &record.plan.plan_id).unwrap();
    cancel_php_install_plan_in_store(&store, &record.plan.plan_id).unwrap();
    let loaded = read_php_install_plans_from_store(&store).unwrap();
    assert_eq!(
        loaded[&record.plan.plan_id].status,
        PhpInstallPlanStatus::Cancelled
    );
    let artifacts = cxszn_menv_core::windows::list_php_artifacts(
        &cxszn_menv_core::windows::embedded_php_catalog().unwrap(),
    )
    .unwrap();
    assert!(summarize_php_install_plans(&loaded, &artifacts, now_epoch_seconds()).is_empty());
    assert_eq!(
        error_payload(
            &reusable_php_install_plan(&loaded, &record.plan.request.idempotency_key).unwrap_err()
        )
        .code,
        "PLAN_CANCELLED",
    );
    let service = Orchestrator::new(EmptyEnvironmentStore);
    let token = service.issue_confirmation_token(&record.plan).unwrap();
    // The caller still holds its pre-cancellation record, as a second process would.
    let error = prepare_php_install_apply_in_store(
        &store,
        &stale[&record.plan.plan_id].plan,
        Some(&token),
        &service,
    )
    .unwrap_err();
    assert_eq!(error_payload(&error).code, "PLAN_CANCELLED");
    assert!(!record.spec.staging_root.exists());
    assert!(!record.spec.managed_directory.exists());
}

#[test]
fn started_and_completed_install_plans_cannot_be_cancelled() {
    let (store, mut record) = fixture();
    let service = Orchestrator::new(EmptyEnvironmentStore);
    let token = service.issue_confirmation_token(&record.plan).unwrap();
    persist_php_install_plans_in_store(
        &store,
        &HashMap::from([(record.plan.plan_id.clone(), record.clone())]),
    )
    .unwrap();
    prepare_php_install_apply_in_store(&store, &record.plan, Some(&token), &service).unwrap();
    assert_eq!(
        cancel_php_install_plan_in_store(&store, &record.plan.plan_id).unwrap_err(),
        ApplicationError::OperationNotCancellable
    );
    record.result = Some(EnvironmentSnapshot::default());
    persist_php_install_plans_in_store(
        &store,
        &HashMap::from([(record.plan.plan_id.clone(), record.clone())]),
    )
    .unwrap();
    assert_eq!(
        cancel_php_install_plan_in_store(&store, &record.plan.plan_id).unwrap_err(),
        ApplicationError::OperationNotCancellable
    );
}

#[test]
fn expired_confirmation_is_rechecked_before_install_is_marked_started() {
    let (store, mut record) = fixture();
    let service = Orchestrator::new(EmptyEnvironmentStore);
    record.plan.expires_at_epoch_seconds = now_epoch_seconds().saturating_sub(1);
    record.plan.plan_hash = install_plan_hash(&record.plan, &record.spec).unwrap();
    persist_php_install_plans_in_store(
        &store,
        &HashMap::from([(record.plan.plan_id.clone(), record.clone())]),
    )
    .unwrap();
    let token = ConfirmationToken {
        value: "expired".to_owned(),
        plan_id: record.plan.plan_id.clone(),
        plan_hash: record.plan.plan_hash.clone(),
        actor: record.plan.request.actor.clone(),
        expires_at_epoch_seconds: record.plan.expires_at_epoch_seconds,
    };
    assert_eq!(
        prepare_php_install_apply_in_store(&store, &record.plan, Some(&token), &service)
            .unwrap_err(),
        ApplicationError::PlanExpired
    );
    assert_eq!(
        read_php_install_plans_from_store(&store).unwrap()[&record.plan.plan_id].status,
        PhpInstallPlanStatus::Pending
    );
}

#[test]
fn install_cancellation_envelopes_match_the_contract_fixture() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../crates/application/schema/fixtures/native-install-plan-cancel.cases.json"
    ))
    .unwrap();
    let (store, record) = fixture();
    let missing = cancel_php_install_plan_in_store(&store, &record.plan.plan_id).unwrap_err();
    let missing = serde_json::to_value(Envelope::<()>::failure(missing)).unwrap();
    assert_eq!(missing["error"]["code"], cases["missingCode"]);
    assert_eq!(missing["ok"], false);
    assert!(!missing["request_id"].as_str().unwrap().is_empty());
    let success = serde_json::to_value(Envelope::success(())).unwrap();
    for key in ["schema_version", "ok", "result"] {
        assert_eq!(success[key], cases["success"][key]);
    }
    assert_eq!(
        serde_json::to_value([
            PhpInstallPlanStatus::Pending,
            PhpInstallPlanStatus::Started,
            PhpInstallPlanStatus::Cancelled
        ])
        .unwrap(),
        cases["statuses"]
    );
    assert_eq!(
        error_payload(&ApplicationError::OperationNotCancellable).code,
        cases["startedOrCompletedCode"]
    );
}

#[test]
fn started_install_retry_preserves_existing_recovery_path() {
    let (store, record) = fixture();
    persist_php_install_plans_in_store(
        &store,
        &HashMap::from([(record.plan.plan_id.clone(), record.clone())]),
    )
    .unwrap();
    let service = Orchestrator::new(EmptyEnvironmentStore);
    let token = service.issue_confirmation_token(&record.plan).unwrap();
    let first =
        prepare_php_install_apply_in_store(&store, &record.plan, Some(&token), &service).unwrap();
    let replay =
        prepare_php_install_apply_in_store(&store, &record.plan, Some(&token), &service).unwrap();
    assert_eq!(first.status, PhpInstallPlanStatus::Started);
    assert_eq!(replay.status, PhpInstallPlanStatus::Started);
    assert_eq!(replay.spec.managed_directory, first.spec.managed_directory);
    assert!(replay.result.is_none());
}

#[test]
fn legacy_install_plan_payload_defaults_to_pending() {
    let (_store, record) = fixture();
    let mut value = serde_json::to_value(record).unwrap();
    value.as_object_mut().unwrap().remove("status");
    let restored: PhpInstallPlanRecord = serde_json::from_value(value).unwrap();
    assert_eq!(restored.status, PhpInstallPlanStatus::Pending);
}
