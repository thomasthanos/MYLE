use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsOptimizationSnapshot {
    pub auto_logon: AutoLogonSnapshot,
    pub firmware_restart: FirmwareRestartSnapshot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FirmwareType {
    Uefi,
    LegacyBios,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareRestartSnapshot {
    pub firmware_type: FirmwareType,
    pub available: bool,
    pub blocked_reason: Option<String>,
    pub active: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FirmwareRestartResult {
    Scheduled,
    Cancelled,
    NeedsAdmin,
    Unsupported,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareRestartOutcome {
    pub result: FirmwareRestartResult,
    pub note: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoLogonStatus {
    Enabled,
    Disabled,
    Conflict,
    Unavailable,
    Working,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoLogonAccountType {
    Local,
    Microsoft,
    Domain,
    Entra,
    Unknown,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoLogonOperation {
    Enable,
    Disable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoLogonSnapshot {
    pub status: AutoLogonStatus,
    pub display_name: String,
    pub account_name: String,
    pub account_type: AutoLogonAccountType,
    pub configured_user: Option<String>,
    pub blocked_reason: Option<String>,
    pub owned_by_app: bool,
    pub active_operation: Option<AutoLogonOperation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoLogonSetResult {
    Enabled,
    Disabled,
    Cancelled,
    NeedsAdmin,
    Conflict,
    Unsupported,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoLogonOutcome {
    pub result: AutoLogonSetResult,
    pub note: Option<String>,
}
