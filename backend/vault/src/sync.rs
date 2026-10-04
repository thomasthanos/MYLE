//! Keeps the vault the same on every device signed in to the account.
//!
//! The account (Supabase) holds the vault's header in `password_vault` and
//! one row per entry in `password_items`, only ever as ciphertext
//! (`docs/supabase/password-manager.sql`, row security per user).
//!
//! Every change is sent with compare-and-swap: a row is replaced only if the
//! account still has the revision this PC last saw. When another PC got
//! there first, the next pass merges the two (`Vault::merge`).

use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::json;

use crate::VaultCell;
use crate::account::Cloud;
use crate::http::err;
use crate::vault::{Record, RemoteHeader, RemoteItem};

const PAGE: usize = 1000;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum SyncResult {
    /// Nobody is signed in: the vault stays on this PC.
    SignedOut,
    /// The account has no vault and neither has this PC.
    Nothing,
    /// Everything is the same here and in the account.
    Synced {
        sent: usize,
        pending: usize,
        /// Entries from the account that did not open with the vault key
        /// (damaged, or not made with it) and were left out.
        rejected: usize,
    },
    /// This PC took the account's vault: open it with its master password.
    Adopted,
    /// The account holds another vault than this PC.
    OtherVault,
}

/// The account's answer when the tables have not been created yet.
fn explain(status: StatusCode, body: &str) -> String {
    if status == StatusCode::NOT_FOUND || body.contains("PGRST205") || body.contains("does not exist") {
        "Password sync is not set up in the account's database yet.".into()
    } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        "The account refused the request. Sign in again.".into()
    } else {
        format!("The account could not sync the passwords ({status}).")
    }
}

async fn send(request: reqwest::RequestBuilder) -> Result<(StatusCode, String), String> {
    let response = request.send().await.map_err(err)?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Ok((status, body))
}

async fn get_header(cloud: &Cloud) -> Result<Option<RemoteHeader>, String> {
    let url = cloud.url(
        "password_vault",
        &[
            ("select", "vault_id,kdf,wrapped_key,recovery_wrapped_key".into()),
            ("user_id", format!("eq.{}", cloud.user_id)),
        ],
    )?;
    let (status, body) = send(cloud.request(Method::GET, url)).await?;
    if !status.is_success() {
        return Err(explain(status, &body));
    }
    let rows: Vec<RemoteHeader> = serde_json::from_str(&body).map_err(err)?;
    Ok(rows.into_iter().next())
}

async fn put_header(cloud: &Cloud, header: &RemoteHeader, exists: bool) -> Result<(), String> {
    let body = json!({
        "user_id": cloud.user_id,
        "vault_id": header.vault_id,
        "kdf": header.kdf,
        "wrapped_key": header.wrapped_key,
        "recovery_wrapped_key": header.recovery_wrapped_key,
    });
    let request = if exists {
        // Only this vault's row: never another vault's by mistake.
        let url = cloud.url(
            "password_vault",
            &[
                ("user_id", format!("eq.{}", cloud.user_id)),
                ("vault_id", format!("eq.{}", header.vault_id)),
            ],
        )?;
        cloud.request(Method::PATCH, url)
    } else {
        cloud.request(Method::POST, cloud.url("password_vault", &[])?)
    };
    let (status, text) = send(request.header("Prefer", "return=minimal").json(&body)).await?;
    if status.is_success() {
        Ok(())
    } else {
        Err(explain(status, &text))
    }
}

async fn get_items(cloud: &Cloud) -> Result<Vec<RemoteItem>, String> {
    let mut items = Vec::new();
    loop {
        let url = cloud.url(
            "password_items",
            &[
                ("select", "id,revision,deleted,ciphertext,updated_at".into()),
                ("user_id", format!("eq.{}", cloud.user_id)),
                ("order", "id".into()),
                ("limit", PAGE.to_string()),
                ("offset", items.len().to_string()),
            ],
        )?;
        let (status, body) = send(cloud.request(Method::GET, url)).await?;
        if !status.is_success() {
            return Err(explain(status, &body));
        }
        let page: Vec<RemoteItem> = serde_json::from_str(&body).map_err(err)?;
        let full = page.len() == PAGE;
        items.extend(page);
        if !full {
            return Ok(items);
        }
    }
}

/// Sends one entry. `false` when another PC changed it first: the next
/// pass merges.
async fn push_item(cloud: &Cloud, record: &Record) -> Result<bool, String> {
    let item = RemoteItem::from_record(record);
    let row = json!({
        "user_id": cloud.user_id,
        "id": item.id,
        "revision": item.revision,
        "deleted": item.deleted,
        "ciphertext": item.ciphertext,
        "updated_at": item.updated_at,
    });
    let (status, body) = match record.base_revision {
        // Never sent: a new row, refused if one exists already.
        None => {
            let url = cloud.url("password_items", &[])?;
            send(cloud.request(Method::POST, url).header("Prefer", "return=minimal").json(&row)).await?
        }
        // Replace only the revision this PC last saw.
        Some(base) => {
            let url = cloud.url(
                "password_items",
                &[
                    ("user_id", format!("eq.{}", cloud.user_id)),
                    ("id", format!("eq.{}", item.id)),
                    ("revision", format!("eq.{base}")),
                ],
            )?;
            send(cloud.request(Method::PATCH, url).header("Prefer", "return=representation").json(&row)).await?
        }
    };
    match status {
        StatusCode::CONFLICT => Ok(false),
        s if s.is_success() => Ok(record.base_revision.is_none() || body.trim() != "[]"),
        s => Err(explain(s, &body)),
    }
}

/// One full pass. Safe to run at any time; while locked it still sends and
/// receives, and merges what needs the key once unlocked.
/// `cloud`: `None` when nobody is signed in.
pub async fn run(cloud: Option<Cloud>, state: &VaultCell) -> Result<SyncResult, String> {
    let Some(cloud) = cloud else {
        return Ok(SyncResult::SignedOut);
    };
    let remote = get_header(&cloud).await?;
    let local = state.with_quiet(|vault| Ok(vault.sync_header()))?;
    match (local, remote) {
        (None, None) => return Ok(SyncResult::Nothing),
        (None, Some(header)) => {
            let items = get_items(&cloud).await?;
            state.with_quiet(|vault| vault.adopt(header, items))?;
            return Ok(SyncResult::Adopted);
        }
        (Some((header, _)), None) => {
            put_header(&cloud, &header, false).await?;
            state.with_quiet(|vault| vault.header_synced())?;
        }
        (Some((local, dirty)), Some(remote)) => {
            if local.vault_id != remote.vault_id {
                // An empty vault here gives way; one with entries asks first.
                if state.with_quiet(|vault| Ok(vault.live_count()))? > 0 {
                    return Ok(SyncResult::OtherVault);
                }
                let items = get_items(&cloud).await?;
                state.with_quiet(|vault| vault.adopt(remote, items))?;
                return Ok(SyncResult::Adopted);
            }
            if dirty {
                put_header(&cloud, &local, true).await?;
                state.with_quiet(|vault| vault.header_synced())?;
            } else if local != remote {
                state.with_quiet(|vault| vault.take_header(remote))?;
            }
        }
    }

    let items = get_items(&cloud).await?;
    let merged = state.with_quiet(|vault| vault.merge(items))?;
    let outgoing = merged.outgoing;
    let mut sent = 0;
    let mut pending = 0;
    for record in &outgoing {
        if push_item(&cloud, record).await? {
            state.with_quiet(|vault| vault.mark_pushed(&record.id, record.revision))?;
            sent += 1;
        } else {
            pending += 1;
        }
    }
    Ok(SyncResult::Synced {
        sent,
        pending,
        rejected: merged.rejected,
    })
}

/// The user chose the account's vault over the one on this PC.
pub async fn use_account_vault(cloud: Option<Cloud>, state: &VaultCell) -> Result<(), String> {
    let cloud = cloud.ok_or("You are not signed in.")?;
    let header = get_header(&cloud).await?.ok_or("Your account has no password vault.")?;
    let items = get_items(&cloud).await?;
    state.with_quiet(|vault| vault.adopt(header, items))
}
