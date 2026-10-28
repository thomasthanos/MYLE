// ! Real changes to Windows, for Windows Sandbox or a throwaway VM only: ! every tweak is applied and undone for real, and everything must be back ! exactly as it was. They refuse to run anywhere else. ! ! In the sandbox, as administrator: ! set MYLE_DEBLOAT_SANDBOX=1 ! myle_lib-<hash>.exe sandbox_ --ignored --test-threads=1 --nocapture
use super::catalog::{self, Hive, Op, TWEAKS};
use super::helper::{self, Request};
use super::system::{self, Value};
use super::undo::{Before, Saved};
use super::{apply_user, detect, start_menu, undo_records, undo_user};
/// Only in Windows Sandbox (its account) or where a VM says so explicitly.
fn throwaway_windows() {
    assert_eq!(
        std::env::var("MYLE_DEBLOAT_SANDBOX").as_deref(),
        Ok("1"),
        "These tests change Windows: run them only in Windows Sandbox or a throwaway VM, with MYLE_DEBLOAT_SANDBOX=1."
    );
    let user = std::env::var("USERNAME").unwrap_or_default();
    assert!(
        user.eq_ignore_ascii_case("WDAGUtilityAccount") || std::env::var("MYLE_DEBLOAT_VM").as_deref() == Ok("1"),
        "Not Windows Sandbox (user {user}); in a VM also set MYLE_DEBLOAT_VM=1."
    );
}
/// Everything the tweaks touch, as it is now.
#[derive(Debug, PartialEq)]
enum Seen {
    Reg { value: Value, key: bool },
    Service(Option<catalog::Start>),
    Task(Option<bool>),
    Clock(String, String),
    Other,
}
fn look(op: &Op) -> Seen {
    match *op {
        Op::Reg { hive, path, name, best_effort, .. } => Seen::Reg {
            value: system::read(hive, path, name),
            // The keys of values Windows may refuse us are its own: it removes HKCU's Feeds key by itself, for one.
            key: best_effort || system::first_missing(hive, path).is_none(),
        },
        Op::Service { name, .. } => Seen::Service(system::start_type(name).unwrap()),
        Op::Task { folder, name } => Seen::Task(system::task_enabled(folder, name).unwrap()),
        Op::Clock24 => {
            let (short, long) = system::time_formats().unwrap();
            Seen::Clock(short, long)
        }
        Op::Appx { .. } | Op::RemoveEdge | Op::Capability { .. } => Seen::Other,
    }
}
fn apply(tweak: &'static catalog::Tweak) -> Vec<Saved> {
    let (mut saved, errors) = apply_user(tweak);
    assert!(errors.is_empty(), "{}: {errors:?}", tweak.id);
    let reply = helper::handle(Request::Apply { tweak: tweak.id.into() });
    assert!(reply.errors.is_empty(), "{}: {:?}", tweak.id, reply.errors);
    saved.extend(reply.saved);
    saved
}
fn undo(tweak: &'static catalog::Tweak, saved: &[Saved]) {
    let (undone, errors) = undo_user(tweak, saved);
    assert!(errors.is_empty(), "{}: {errors:?}", tweak.id);
    let machine: Vec<Saved> = saved.iter().filter(|record| !undone.contains(&record.op)).cloned().collect();
    let reply = helper::handle(Request::Undo { tweak: tweak.id.into(), saved: machine.clone() });
    assert!(reply.errors.is_empty(), "{}: {:?}", tweak.id, reply.errors);
    assert_eq!(reply.undone.len() + undone.len(), saved.len(), "{}: everything kept was put back", tweak.id);
}
/// The tweaks for this Windows, but Edge and the apps (a test of their own).
fn plain_tweaks(build: u32) -> Vec<&'static catalog::Tweak> {
    TWEAKS
        .iter()
        .filter(|tweak| tweak.builds.contains(build) && !tweak.ops.iter().any(|op| matches!(op, Op::RemoveEdge | Op::Appx { .. } | Op::Capability { .. })))
        .collect()
}
fn snapshot(tweaks: &[&'static catalog::Tweak]) -> Vec<Vec<Seen>> {
    tweaks.iter().map(|tweak| tweak.ops.iter().map(look).collect()).collect()
}
fn assert_as_before(tweaks: &[&'static catalog::Tweak], before: &[Vec<Seen>]) {
    for ((tweak, was), is) in tweaks.iter().zip(before).zip(snapshot(tweaks)) {
        for (index, (was, is)) in was.iter().zip(&is).enumerate() {
            assert_eq!(was, is, "{} op {index} is not as it was", tweak.id);
        }
        println!("{:<24} back exactly as it was", tweak.id);
    }
}
#[test]
#[ignore = "changes Windows: Windows Sandbox or a throwaway VM only"]
fn sandbox_every_tweak_applies_and_undoes_exactly() {
    throwaway_windows();
    let build = system::windows_info().build;
    let tweaks = plain_tweaks(build);
    let before = snapshot(&tweaks);
    let mut kept = Vec::new();
    for tweak in &tweaks {
        let saved = apply(tweak);
        let state = detect::tweak_state(tweak, build, &system::installed_packages());
        println!("{:<24} applied: {state:?} ({} kept)", tweak.id, saved.len());
        assert_eq!(state, detect::State::Applied, "{}", tweak.id);
        kept.push(saved);
    }
    // Applying again changes nothing and keeps nothing new.
    for tweak in &tweaks {
        assert!(apply(tweak).is_empty(), "{} applied twice", tweak.id);
    }
    for (tweak, saved) in tweaks.iter().zip(&kept) {
        undo(tweak, saved);
    }
    assert_as_before(&tweaks, &before);
}
/// A tweak that was on before MYLE ran (MYLE kept nothing for it) is turned
/// off with the Windows defaults, and the administrator helper takes them.
#[test]
#[ignore = "changes Windows: Windows Sandbox or a throwaway VM only"]
fn sandbox_a_tweak_on_before_myle_turns_off() {
    throwaway_windows();
    let build = system::windows_info().build;
    let packages = system::installed_packages();
    let tweaks = plain_tweaks(build);
    let before = snapshot(&tweaks);
    let kept: Vec<Vec<Saved>> = tweaks.iter().map(|tweak| apply(tweak)).collect();
    let mut was_on = Vec::new();
    for tweak in &tweaks {
        let records = undo_records(tweak, Vec::new(), &packages);
        let on: Vec<Saved> = records
            .iter()
            .map(|record| Saved { op: record.op, before: as_it_is(&tweak.ops[record.op]) })
            .collect();
        undo(tweak, &records);
        for record in &records {
            let op = &tweak.ops[record.op];
            assert_eq!(detect::op_applied(op, &packages), Some(false), "{} op {} is still on", tweak.id, record.op);
        }
        println!("{:<24} turned off with {} defaults", tweak.id, records.len());
        was_on.push(on);
    }
    // Back to how this Windows was: on again as it was, last first, then what the first apply kept.
    for (tweak, on) in tweaks.iter().zip(&was_on).rev() {
        undo(tweak, on);
    }
    for (tweak, saved) in tweaks.iter().zip(&kept) {
        undo(tweak, saved);
    }
    assert_as_before(&tweaks, &before);
}
/// An operation's part as it is now, to be put back later by an undo.
fn as_it_is(op: &Op) -> Before {
    match *op {
        Op::Reg { hive, path, name, .. } => Before::Reg { value: system::read(hive, path, name), created: None },
        Op::Service { name, .. } => Before::Service { start: system::start_type(name).unwrap().unwrap() },
        Op::Task { folder, name } => Before::Task { enabled: system::task_enabled(folder, name).unwrap().unwrap() },
        Op::Clock24 => {
            let (short, long) = system::time_formats().unwrap();
            Before::Clock { short, long }
        }
        Op::Appx { .. } | Op::RemoveEdge | Op::Capability { .. } => unreachable!("not among the plain tweaks"),
    }
}
/// The Start menu's policies the administrator helper sets, and only those.
#[test]
#[ignore = "changes Windows: Windows Sandbox or a throwaway VM only"]
fn sandbox_start_menu_policies_come_and_go() {
    throwaway_windows();
    const VALUES: [(&str, &str); 6] = [
        (r"SOFTWARE\Policies\Microsoft\Windows\Explorer", "HideRecommendedSection"),
        (r"SOFTWARE\Microsoft\PolicyManager\current\device\Start", "HideRecommendedSection"),
        (r"SOFTWARE\Microsoft\PolicyManager\current\device\Education", "IsEducationEnvironment"),
        (r"SOFTWARE\Microsoft\PolicyManager\current\device\Start", "ConfigureStartPins"),
        (r"SOFTWARE\Microsoft\PolicyManager\current\device\Start", "ConfigureStartPins_ProviderSet"),
        (
            r"SOFTWARE\Microsoft\PolicyManager\providers\B5292708-1619-419B-9923-E5D9F3925E71\default\Device\Start",
            "ConfigureStartPins",
        ),
    ];
    let look_all = || VALUES.map(|(path, name)| system::read(Hive::Machine, path, name));
    let before = look_all();
    assert!(helper::handle(Request::SetHideRecommended { hide: true }).errors.is_empty());
    assert!(start_menu::status().hide_recommended);
    assert!(helper::handle(Request::SetHideRecommended { hide: false }).errors.is_empty());
    let json = start_menu::build_pins_json(&["explorer".into(), "settings".into(), "terminal".into()]).unwrap();
    assert!(helper::handle(Request::SetStartPins { json: Some(json.clone()) }).errors.is_empty());
    assert_eq!(look_all()[3], Value::Sz { value: json });
    assert!(helper::handle(Request::SetStartPins { json: None }).errors.is_empty());
    let forged = r#"{"pinnedList":[{"desktopAppId":"C:\\evil.exe"}]}"#.to_string();
    assert!(!helper::handle(Request::SetStartPins { json: Some(forged) }).errors.is_empty());
    assert_eq!(look_all(), before, "nothing is left behind");
}
#[test]
#[ignore = "changes Windows: Windows Sandbox or a throwaway VM only"]
fn sandbox_edge_goes_and_webview2_stays() {
    throwaway_windows();
    // Windows Sandbox shares Edge's files with the host, read-only: Edge's uninstaller is allowed there (its log says "Uninstall allowed") and removes the shortcuts and the MSIX, but then retries every file for ~2 s each and runs out of time. Only a VM can show Edge really gone.
    if std::env::var("MYLE_DEBLOAT_VM").as_deref() != Ok("1") {
        println!("Windows Sandbox cannot delete Edge's files: run this one in a VM (MYLE_DEBLOAT_VM=1).");
        return;
    }
    if !system::edge_installed() {
        println!("Edge is not installed here: nothing to test.");
        return;
    }
    let webview = system::program_files_x86().join(r"Microsoft\EdgeWebView\Application");
    let had_webview = webview.is_dir();
    let edge = catalog::find("edge").unwrap();
    let started = std::time::Instant::now();
    let reply = helper::handle(Request::Apply { tweak: edge.id.into() });
    println!("Edge's uninstaller took {:?}", started.elapsed());
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(!system::edge_installed(), "Edge is gone");
    assert_eq!(webview.is_dir(), had_webview, "WebView2 stays");
    let allow = system::read(Hive::Machine, r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdateDev", "AllowUninstall");
    assert_eq!(allow, Value::Absent, "the uninstall permission is taken back");
}
/// A part of Windows is removed through the helper, seen as gone without
/// administrator rights, and added back (from Windows Update).
#[test]
#[ignore = "changes Windows: Windows Sandbox or a throwaway VM only"]
fn sandbox_a_windows_feature_goes_and_comes_back() {
    throwaway_windows();
    let Some((tweak, package)) = ["math-input", "steps-recorder", "powershell-ise"].iter().find_map(|id| {
        let tweak = catalog::find(id).unwrap();
        let Op::Capability { package, .. } = tweak.ops[0] else { unreachable!() };
        (system::capability_installed(package) == Some(true)).then_some((tweak, package))
    }) else {
        println!("None of the test features is installed here: nothing to test.");
        return;
    };
    println!("{}: installed", tweak.id);
    let started = std::time::Instant::now();
    let removed = helper::handle(Request::Apply { tweak: tweak.id.into() });
    println!("removed in {:?}: {:?}", started.elapsed(), removed.errors);
    assert!(removed.errors.is_empty(), "{:?}", removed.errors);
    assert_eq!(removed.saved.len(), 1, "the removal is kept for Undo");
    assert_eq!(removed.saved[0].before, Before::Capability);
    assert_eq!(system::capability_installed(package), Some(false), "seen as gone, without rights");
    let packages = system::installed_packages();
    assert_eq!(detect::tweak_state(tweak, system::windows_info().build, &packages), detect::State::Applied);
    let started = std::time::Instant::now();
    let back = helper::handle(Request::Undo { tweak: tweak.id.into(), saved: removed.saved });
    println!("added back in {:?}: {:?}", started.elapsed(), back.errors);
    assert!(back.errors.is_empty(), "{:?}", back.errors);
    assert_eq!(system::capability_installed(package), Some(true), "installed again");
}
#[test]
#[ignore = "changes Windows: Windows Sandbox or a throwaway VM only"]
fn sandbox_a_restore_point_is_made() {
    throwaway_windows();
    let reply = helper::handle(Request::RestorePoint { turn_on: true });
    println!("{:?}", reply.restore_point);
    assert!(reply.restore_point.is_some());
}
