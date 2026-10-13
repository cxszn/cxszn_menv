use cxszn_menv_core::application::{ApplicationError, ChangePlan};
use cxszn_menv_core::domain::RuntimeRecord;
use cxszn_menv_core::ports::PortError;
use std::io::{BufRead, Read, Write};

pub(super) fn require_interactive_terminal(
    stdin_console: bool,
    stderr_console: bool,
) -> Result<(), ApplicationError> {
    if !stdin_console || !stderr_console {
        return Err(ApplicationError::ConfirmationRequired);
    }
    Ok(())
}

pub(super) fn require_explicit_approval(plan_id: &str, line: &str) -> Result<(), ApplicationError> {
    if uuid::Uuid::parse_str(plan_id).is_err()
        || line.len() > 256
        || line.trim() != format!("approve {plan_id}")
    {
        return Err(ApplicationError::ConfirmationRequired);
    }
    Ok(())
}

pub(super) fn confirmation_summary(
    plan: &ChangePlan,
    verified_runtime: Option<&RuntimeRecord>,
) -> serde_json::Value {
    serde_json::json!({
        "planId": plan.plan_id, "planHash": plan.plan_hash,
        "action": plan.request.action, "family": plan.request.family,
        "runtimeId": plan.request.runtime_id, "targetRuntimeId": plan.request.target_runtime_id,
        "actor": plan.request.actor, "backupSource": plan.backup_source,
        "verifiedRuntime": verified_runtime,
        "expiresAtEpochSeconds": plan.expires_at_epoch_seconds,
    })
}

pub(super) fn read_user_confirmation(
    plan: &ChangePlan,
    verified_runtime: Option<&RuntimeRecord>,
) -> Result<(), ApplicationError> {
    let capabilities = cxszn_menv_core::windows::console::confirmation_console_capabilities();
    require_interactive_terminal(capabilities.stdin_console, capabilities.stderr_console)?;
    let summary = confirmation_summary(plan, verified_runtime);
    let mut prompt = std::io::stderr().lock();
    // JSON escaping keeps filenames/actors from injecting terminal controls.
    writeln!(prompt, "请检查环境变更计划：{summary}")
        .and_then(|_| {
            writeln!(
                prompt,
                "确认请输入 approve {}；其它输入取消：",
                plan.plan_id
            )
        })
        .and_then(|_| prompt.flush())
        .map_err(|_| PortError::Platform("confirmation prompt could not be displayed".into()))?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .take(257)
        .read_line(&mut line)
        .map_err(|_| PortError::Platform("confirmation input could not be read".into()))?;
    require_explicit_approval(&plan.plan_id, &line)
}
