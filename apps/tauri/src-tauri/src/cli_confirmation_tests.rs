use super::cli_confirmation::{
    confirmation_summary, require_explicit_approval, require_interactive_terminal,
};

const PLAN_ID: &str = "72b9d059-77b3-4ecb-9d89-1bda3da5b840";
const OTHER_PLAN_ID: &str = "c7e69647-00ed-4f39-9855-cb5c50ef0f1b";

#[test]
fn approval_prompt_requires_both_stdin_and_stderr_to_be_interactive() {
    for (stdin_is_terminal, stderr_is_terminal, permitted) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        assert_eq!(
            require_interactive_terminal(stdin_is_terminal, stderr_is_terminal).is_ok(),
            permitted,
            "stdin TTY={stdin_is_terminal}, stderr TTY={stderr_is_terminal}"
        );
    }
}

#[test]
fn exact_approval_of_the_full_plan_uuid_accepts_only_surrounding_whitespace() {
    for line in [
        format!("approve {PLAN_ID}"),
        format!(" \tapprove {PLAN_ID}\r\n "),
    ] {
        assert!(require_explicit_approval(PLAN_ID, &line).is_ok());
    }
}

#[test]
fn generic_affirmatives_and_empty_input_cannot_authorize_a_plan() {
    for line in [
        "",
        " \t\r\n ",
        "yes",
        "y",
        "true",
        "confirm=true",
        "--yes",
        "approve",
        "approve <full UUID>",
        "{\"confirm\":true}",
    ] {
        assert!(
            require_explicit_approval(PLAN_ID, line).is_err(),
            "approval must include the exact plan UUID"
        );
    }
}

#[test]
fn approval_of_another_or_incomplete_plan_uuid_is_rejected() {
    for line in [
        format!("approve {OTHER_PLAN_ID}"),
        format!("approve {}", &PLAN_ID[..8]),
        format!("approve {}", PLAN_ID.to_uppercase()),
        format!("approve {{{PLAN_ID}}}"),
    ] {
        assert!(require_explicit_approval(PLAN_ID, &line).is_err());
    }
    for invalid_plan_id in ["", "72b9d059", "../not-a-uuid"] {
        assert!(
            require_explicit_approval(invalid_plan_id, &format!("approve {invalid_plan_id}"))
                .is_err()
        );
    }
}

#[test]
fn approval_with_extra_commands_or_nonexact_spacing_is_rejected() {
    for line in [
        format!("approve {PLAN_ID}; extra-command"),
        format!("approve {PLAN_ID}\nextra-command"),
        format!("approve {PLAN_ID} extra-argument"),
        format!("approve {PLAN_ID} --yes"),
        format!("approve  {PLAN_ID}"),
        format!("approve\t{PLAN_ID}"),
        format!("APPROVE {PLAN_ID}"),
        format!("approve {PLAN_ID}\0"),
    ] {
        assert!(require_explicit_approval(PLAN_ID, &line).is_err());
    }
}

#[test]
fn approval_input_limit_is_checked_before_whitespace_trimming() {
    let approval = format!("approve {PLAN_ID}");
    let at_limit = format!("{}{}", " ".repeat(256 - approval.len()), approval);
    assert_eq!(at_limit.len(), 256);
    assert!(require_explicit_approval(PLAN_ID, &at_limit).is_ok());

    let over_limit = format!(" {at_limit}");
    assert_eq!(over_limit.len(), 257);
    assert!(require_explicit_approval(PLAN_ID, &over_limit).is_err());
    assert!(require_explicit_approval(PLAN_ID, &"x".repeat(257)).is_err());
}

#[test]
fn registration_prompt_displays_verified_payload_even_when_runtime_id_is_unchanged() {
    use cxszn_menv_core::application::{ChangeAction, ChangePlan, ChangeRequest};
    use cxszn_menv_core::domain::{
        Architecture, RuntimeFamily, RuntimeId, RuntimeRecord, RuntimeSource,
    };

    let mut runtime = RuntimeRecord::discovered(
        RuntimeId::new("unchanged-display-id").expect("runtime id"),
        RuntimeFamily::Php,
        "8.5.11",
        Architecture::X64,
        r"C:\verified-one\php.exe",
        RuntimeSource::Manual,
    );
    let now = super::now_epoch_seconds();
    let mut plan = ChangePlan {
        plan_id: PLAN_ID.to_owned(),
        plan_hash: String::new(),
        request: ChangeRequest {
            action: ChangeAction::Register,
            family: RuntimeFamily::Php,
            runtime_id: Some(runtime.id.clone()),
            target_runtime_id: None,
            idempotency_key: "prompt-payload-binding".to_owned(),
            actor: "user\n\u{1b}[31m".to_owned(),
        },
        requires_confirmation: true,
        created_at_epoch_seconds: now,
        expires_at_epoch_seconds: now + 300,
        snapshot_hash: "fixture-snapshot".to_owned(),
        backup_source: None,
    };

    for path in [r"C:\verified-one\php.exe", r"C:\verified-two\php.exe"] {
        runtime.executable_path = path.to_owned();
        plan.plan_hash = super::registration_plan_hash(&plan, &runtime).expect("bound payload");
        super::validate_native_plan_payload(&plan, Some(&runtime)).expect("valid bound plan");
        let summary = confirmation_summary(&plan, Some(&runtime));
        assert_eq!(summary["runtimeId"], "unchanged-display-id");
        assert_eq!(summary["verifiedRuntime"]["executablePath"], path);
        assert_eq!(
            summary["verifiedRuntime"],
            serde_json::to_value(&runtime).expect("runtime JSON")
        );
        let rendered = summary.to_string();
        assert!(!rendered.contains('\n'));
        assert!(!rendered.contains('\u{1b}'));
        assert!(!rendered.contains("confirmationToken"));
    }
}
