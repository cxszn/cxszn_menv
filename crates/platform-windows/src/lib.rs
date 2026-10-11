//! Windows adapter boundary.
//!
//! The first slice intentionally provides contract-safe placeholders. Real
//! registry/PATH, ConPTY, and process probing code belongs here and must stay
//! behind the `RuntimeProvider`, `UserPathStore`, and `ProcessProbe` ports.

use cxszn_menv_domain::{RuntimeFamily, RuntimeRecord};
use cxszn_menv_ports::{PortError, RuntimeProvider, ScanRequest, VerifyPathRequest};

#[derive(Default)]
pub struct WindowsPhpProvider;

impl RuntimeProvider for WindowsPhpProvider {
    fn family(&self) -> RuntimeFamily {
        RuntimeFamily::Php
    }

    fn scan(&self, _request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
        // Discovery is intentionally not implemented until Windows probing is
        // covered by integration tests on a Windows 11 host.
        Err(PortError::UnsupportedPlatform)
    }

    fn verify_path(&self, _request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
        Err(PortError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cxszn_menv_ports::RuntimeProvider;

    #[test]
    fn placeholder_does_not_claim_discovery() {
        let provider = WindowsPhpProvider;
        assert_eq!(provider.family(), RuntimeFamily::Php);
        assert_eq!(
            provider.scan(&ScanRequest::default()),
            Err(PortError::UnsupportedPlatform)
        );
    }
}
