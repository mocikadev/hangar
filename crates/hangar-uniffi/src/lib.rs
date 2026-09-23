//! UniFFI adapter for the native macOS application.
//!
//! This crate owns exported records, errors, and task handles. It converts from
//! `hangar-core` types and never exports credentials or platform UI objects.

mod dto;
mod service;

pub use dto::*;
pub use service::HangarService;

uniffi::setup_scaffolding!();

pub const INTERFACE_VERSION: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BridgeInfo {
    pub interface_version: u32,
    pub core_version: String,
}

#[uniffi::export]
pub fn bridge_info() -> BridgeInfo {
    BridgeInfo {
        interface_version: INTERFACE_VERSION,
        core_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_contract_has_explicit_version() {
        let info = bridge_info();
        assert_eq!(info.interface_version, 4);
        assert!(!info.core_version.is_empty());
    }
}
