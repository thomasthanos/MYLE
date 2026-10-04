//! The Password Manager's core, shared by the Windows app (`backend/`) and
//! MYLE Passwords for phones (`backend/mobile/`): the vault and its
//! encryption, 2FA codes, passkeys, imports, website icons, and keeping the
//! vault the same on every device through the account (Supabase).
//!
//! Nothing here touches the operating system beyond files and the network;
//! each app brings its own storage for the account's session, its clipboard
//! and its unlock with Windows Hello or biometrics.

pub mod account;
pub mod cell;
pub mod crypto;
pub mod generator;
pub mod http;
pub mod icons;
pub mod import;
pub mod passkeys;
pub mod qr;
pub mod sites;
pub mod sync;
pub mod totp;
pub mod vault;

pub use cell::VaultCell;
