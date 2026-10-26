//! The fast scan's administrator half. Reading a volume's file table needs an
//! administrator, so the elevated helper reads it and sends the app a
//! snapshot of names, folders and sizes (`mft::Snapshot`). The app asks by
//! drive letter only; the helper reads only a fixed drive's NTFS table, and
//! never hands over the volume itself, which would let the app read any
//! file's contents.

use serde::{Deserialize, Serialize};

use super::mft;
use crate::elevated_pipe::{self, Helper, Session};

/// `\\.\pipe\myle-disk-<32 hex digits>`, created by the app.
const HELPER: Helper = Helper {
    flag: "--disk-analyzer-elevated-helper",
    pipe_prefix: r"\\.\pipe\myle-disk-",
    what: "administrator scan",
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Request {
    /// The file table of this drive ("C").
    Scan { drive: char },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Response {
    /// The snapshot follows, as bytes.
    Ok { elapsed_ms: u64 },
    Error { message: String },
}

/// Where a fast scan is, for the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Windows is asking for administrator approval.
    Approval,
    /// The helper is reading the file table.
    Table,
}

/// The running helper: one UAC prompt, then every fast scan of this app
/// session goes through it (until it has been idle for half an hour).
static SESSION: tokio::sync::Mutex<Option<Session>> = tokio::sync::Mutex::const_new(None);

/// Reads drive `letter`'s file table through the helper, starting it (and
/// asking for approval) when none runs. The snapshot's bytes
/// (`mft::Snapshot::decode`) and how long the reading took, in ms.
pub async fn read(letter: char, mut on_phase: impl FnMut(Phase)) -> Result<(Vec<u8>, u64), String> {
    let request = Request::Scan { drive: letter };
    let mut session = SESSION.lock().await;
    if let Some(open) = session.as_mut() {
        on_phase(Phase::Table);
        match open.ask_with_payload(&request).await {
            Ok(answer) => return answer_of(answer),
            // The helper has gone: start a new one below.
            Err(_) => *session = None,
        }
    }
    on_phase(Phase::Approval);
    let mut fresh = Session::start(&HELPER).await?;
    on_phase(Phase::Table);
    let answer = fresh
        .ask_with_payload(&request)
        .await
        .map_err(|error| format!("The administrator scan stopped: {error}"))?;
    *session = Some(fresh);
    answer_of(answer)
}

fn answer_of((response, payload): (Response, Vec<u8>)) -> Result<(Vec<u8>, u64), String> {
    match response {
        Response::Ok { elapsed_ms } => Ok((payload, elapsed_ms)),
        Response::Error { message } => Err(message),
    }
}

/// Called before Tauri starts. `None` for a normal launch, an exit code for
/// the helper.
pub fn run_helper_from_args() -> Option<i32> {
    let args = elevated_pipe::helper_args(&HELPER)?;
    let served = elevated_pipe::parse_serve_args(&HELPER, &args).and_then(|(pipe, app_pid)| serve(&pipe, app_pid));
    Some(match served {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Disk Analyzer helper: {error}");
            2
        }
    })
}

fn serve(pipe: &str, app_pid: u32) -> Result<(), String> {
    elevated_pipe::serve_with_payload(&HELPER, pipe, app_pid, handle, || {
        (Response::Error { message: "Invalid scan request.".into() }, Vec::new())
    })
}

fn handle(request: Request) -> (Response, Vec<u8>) {
    let Request::Scan { drive } = request;
    let started = std::time::Instant::now();
    match mft::read_volume(drive) {
        Ok(snapshot) => (Response::Ok { elapsed_ms: started.elapsed().as_millis() as u64 }, snapshot.encode()),
        Err(message) => (Response::Error { message }, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::net::windows::named_pipe::ServerOptions;

    use super::*;
    use crate::elevated_pipe::tests::{connect_to, runtime};

    #[test]
    fn the_helper_answers_by_drive_letter_only() {
        runtime().block_on(async {
            let name = format!("{}{}", HELPER.pipe_prefix, uuid::Uuid::new_v4().simple());
            let server = ServerOptions::new().first_pipe_instance(true).create(&name).unwrap();
            let helper = {
                let name = name.clone();
                std::thread::spawn(move || serve(&name, std::process::id()))
            };
            let mut session = connect_to(server).await;

            let answer: (Response, Vec<u8>) = session.ask_with_payload(&Request::Scan { drive: '1' }).await.unwrap();
            assert!(matches!(answer, (Response::Error { message }, bytes) if message == "That is not a drive." && bytes.is_empty()));
            // Anything else, a path above all, is not a request.
            let path = serde_json::json!({ "op": "scan", "drive": r"C:\Windows" });
            let answer: (Response, Vec<u8>) = session.ask_with_payload(&path).await.unwrap();
            assert!(matches!(answer.0, Response::Error { message } if message == "Invalid scan request."));

            drop(session);
            let ended = tokio::task::spawn_blocking(move || helper.join().unwrap());
            let ended = tokio::time::timeout(Duration::from_secs(20), ended)
                .await
                .expect("the helper did not end when the pipe closed")
                .unwrap();
            assert!(ended.is_ok());
        });
    }
}
