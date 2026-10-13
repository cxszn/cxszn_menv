use cxszn_menv_core::domain::{
    Architecture, Availability, PhpThreadSafety, RuntimeFamily, RuntimeRecord,
};
use cxszn_menv_core::windows::PhpArtifactSummary;

pub(crate) fn mark_installed(artifacts: &mut [PhpArtifactSummary], runtimes: &[RuntimeRecord]) {
    for artifact in artifacts {
        artifact.installed = runtimes.iter().any(|runtime| {
            artifact.family == "php"
                && runtime.family == RuntimeFamily::Php
                && runtime.version.as_deref() == Some(artifact.version.as_str())
                && match artifact.architecture.as_str() {
                    "x86_64" | "x64" => runtime.architecture == Architecture::X64,
                    "x86" => runtime.architecture == Architecture::X86,
                    "arm64" => runtime.architecture == Architecture::Arm64,
                    _ => false,
                }
                && match artifact.thread_safety.as_str() {
                    "ts" => runtime.thread_safety == PhpThreadSafety::Ts,
                    "nts" => runtime.thread_safety == PhpThreadSafety::Nts,
                    _ => false,
                }
                && matches!(
                    runtime.availability,
                    Availability::DetectedUnmanaged | Availability::Manual | Availability::Managed
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cxszn_menv_core::domain::{RuntimeId, RuntimeSource};

    fn catalog_artifact() -> PhpArtifactSummary {
        PhpArtifactSummary {
            family: "php".into(),
            artifact_id: "php-8.5.11-nts-x64".into(),
            version: "8.5.11".into(),
            architecture: "x86_64".into(),
            thread_safety: "nts".into(),
            lifecycle: "supported".into(),
            legacy: false,
            archival: false,
            file_name: "fixture.zip".into(),
            size_bytes: 1,
            installed: false,
        }
    }

    fn runtime(architecture: Architecture, thread_safety: PhpThreadSafety) -> RuntimeRecord {
        let mut runtime = RuntimeRecord::discovered(
            RuntimeId::new("fixture-php").expect("id"),
            RuntimeFamily::Php,
            "8.5.11",
            architecture,
            r"C:\fixture\php.exe",
            RuntimeSource::Manual,
        );
        runtime.thread_safety = thread_safety;
        runtime
    }

    #[test]
    fn ts_installation_does_not_hide_catalog_nts_download() {
        let mut artifacts = vec![catalog_artifact()];
        mark_installed(
            &mut artifacts,
            &[runtime(Architecture::X64, PhpThreadSafety::Ts)],
        );
        assert!(!artifacts[0].installed);
    }

    #[test]
    fn architecture_variant_must_match_the_catalog_artifact() {
        let mut artifact = catalog_artifact();
        artifact.architecture = "x86".into();
        let mut artifacts = vec![artifact];
        mark_installed(
            &mut artifacts,
            &[runtime(Architecture::X64, PhpThreadSafety::Nts)],
        );
        assert!(!artifacts[0].installed);
    }

    #[test]
    fn matching_build_is_installed_but_invalid_and_unknown_builds_are_not() {
        let mut artifacts = vec![catalog_artifact()];
        let mut local = runtime(Architecture::X64, PhpThreadSafety::Nts);
        mark_installed(&mut artifacts, &[local.clone()]);
        assert!(artifacts[0].installed);
        local.availability = Availability::InvalidPath;
        mark_installed(&mut artifacts, &[local.clone()]);
        assert!(!artifacts[0].installed);
        local.availability = Availability::Manual;
        local.thread_safety = PhpThreadSafety::Unknown;
        mark_installed(&mut artifacts, &[local]);
        assert!(!artifacts[0].installed);
    }
}
