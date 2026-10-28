// ! Where each tweak stands on this PC, read without administrator rights: ! the registry, services, scheduled tasks, the time format and the ! installed Store apps.
use serde::Serialize;
use super::catalog::{self, Data, Op, Tweak};
use super::system::{self, Value};
use super::undo::{Before, Store};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Applied,
    NotApplied,
    /// Some of it is in place.
    Partial,
    /// Not for this version of Windows.
    Unavailable,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweakStatus {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub category: catalog::Category,
    pub risk: catalog::Risk,
    pub level: Option<catalog::Level>,
    pub note: Option<&'static str>,
    /// Complete only after Windows restarts.
    pub restart: bool,
    pub confirm: Option<&'static str>,
    pub state: State,
    /// MYLE applied it and kept what was there before.
    pub can_undo: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub id: &'static str,
    pub title: &'static str,
    pub group: catalog::AppGroup,
    pub level: Option<catalog::Level>,
    pub about: &'static str,
    pub keep: Option<&'static str>,
    /// Its packages installed for this user; empty when it is not.
    pub packages: Vec<String>,
    /// Removed by MYLE: it can be installed again from the Store.
    pub removed_by_myle: bool,
    pub store_id: Option<&'static str>,
}
/// One operation's standing: `None` when it does not apply here (a
/// service or task this PC does not have, or one the user set by hand).
pub(super) fn op_applied(op: &Op, packages: &[String]) -> Option<bool> {
    match *op {
        Op::Reg { best_effort: true, .. } => None,
        Op::Reg { hive, path, name, data, .. } => Some(system::read(hive, path, name).matches(data)),
        Op::Service { name, to, from, .. } => {
            let current = system::start_type(name).ok()??;
            if current.rank() >= to.rank() {
                Some(true)
            } else if from.contains(&current) {
                Some(false)
            } else {
                None
            }
        }
        Op::Task { folder, name } => system::task_enabled(folder, name).ok()?.map(|enabled| !enabled),
        Op::Appx { app } => {
            let name = catalog::Name::Exact(app);
            Some(!packages.iter().any(|package| name.matches(package)))
        }
        Op::Clock24 => system::time_formats().ok().map(|(short, _)| system::is_24h(&short)),
        Op::RemoveEdge => Some(!system::edge_installed()),
        Op::Capability { package, .. } => system::capability_installed(package).map(|installed| !installed),
    }
}
/// The Windows default ("Off" state) for an operation when turning off a
/// tweak that was already in place on this PC before MYLE ran.
pub(super) fn default_before(op: &Op) -> Option<Before> {
    match *op {
        Op::Reg { path, name, data, .. } => {
            if path.contains(r"Policies\") || path.ends_with(r"Siuf\Rules") {
                return Some(Before::Reg { value: Value::Absent, created: None });
            }
            if path.ends_with("InprocServer32") {
                return Some(Before::Reg {
                    value: Value::Absent,
                    created: Some(r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}".into()),
                });
            }
            let value = match (name, data) {
                ("Hidden", Data::Dword(1)) => Value::Dword { value: 2 },
                ("ShellFeedsTaskbarViewMode", Data::Dword(2)) => Value::Dword { value: 0 },
                ("CortanaConsent", Data::Dword(0)) => Value::Absent,
                (_, Data::Dword(0)) => Value::Dword { value: 1 },
                (_, Data::Dword(1)) => Value::Dword { value: 0 },
                ("Value", Data::Sz("Deny")) => Value::Sz { value: "Allow".into() },
                ("MouseSpeed", Data::Sz("0")) => Value::Sz { value: "1".into() },
                ("MouseThreshold1", Data::Sz("0")) => Value::Sz { value: "6".into() },
                ("MouseThreshold2", Data::Sz("0")) => Value::Sz { value: "10".into() },
                ("Flags", Data::Sz("506")) => Value::Sz { value: "510".into() },
                _ => Value::Absent,
            };
            Some(Before::Reg { value, created: None })
        }
        // Never a service Windows itself keeps off (Remote Registry).
        Op::Service { to, windows, .. } => (windows != to).then_some(Before::Service { start: windows }),
        Op::Task { .. } => Some(Before::Task { enabled: true }),
        Op::Clock24 => {
            let (short, long) = system::time_formats().ok()?;
            Some(Before::Clock {
                short: system::to_12h(&short),
                long: system::to_12h(&long),
            })
        }
        Op::RemoveEdge => Some(Before::Edge),
        Op::Capability { .. } => Some(Before::Capability),
        Op::Appx { .. } => None,
    }
}
pub fn tweak_state(tweak: &Tweak, build: u32, packages: &[String]) -> State {
    if !tweak.builds.contains(build) {
        return State::Unavailable;
    }
    let states: Vec<bool> = tweak.ops.iter().filter_map(|op| op_applied(op, packages)).collect();
    // A part of Windows this version does not have at all.
    if states.is_empty() && tweak.category == catalog::Category::Features {
        return State::Unavailable;
    }
    match (states.iter().filter(|&&on| on).count(), states.len()) {
        (_, 0) => State::Applied,
        (on, all) if on == all => State::Applied,
        (0, _) => State::NotApplied,
        _ => State::Partial,
    }
}
pub fn tweaks(build: u32, packages: &[String], store: &Store) -> Vec<TweakStatus> {
    catalog::TWEAKS
        .iter()
        .map(|tweak| TweakStatus {
            id: tweak.id,
            title: tweak.title,
            summary: tweak.summary,
            category: tweak.category,
            risk: tweak.risk,
            level: tweak.level,
            note: tweak.note,
            restart: tweak.reboot,
            confirm: tweak.confirm,
            state: match tweak_state(tweak, build, packages) {
                // Gone from the servicing store once removed: MYLE knows it did it.
                State::Unavailable if tweak.category == catalog::Category::Features && store.tweaks.contains_key(tweak.id) => State::Applied,
                state => state,
            },
            can_undo: store.tweaks.contains_key(tweak.id),
        })
        .collect()
}
pub fn apps(packages: &[String], store: &Store) -> Vec<AppStatus> {
    catalog::APPS
        .iter()
        .map(|app| AppStatus {
            id: app.id,
            title: app.title,
            group: app.group,
            level: app.level,
            about: app.about,
            keep: app.keep,
            packages: packages
                .iter()
                .filter(|package| app.name.matches(package) && catalog::removable(package).is_some_and(|found| found.id == app.id))
                .cloned()
                .collect(),
            removed_by_myle: store.removed_apps.contains(app.id),
            store_id: app.store_id,
        })
        .collect()
}
/// Whether a tweak changes anything the administrator helper must do.
pub fn needs_admin(tweak: &Tweak) -> bool {
    tweak.ops.iter().any(|op| !op.as_user())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_tweak_has_a_state_on_this_pc() {
        let info = system::windows_info();
        let packages = system::installed_packages();
        let store = Store::default();
        let all = tweaks(info.build, &packages, &store);
        assert_eq!(all.len(), catalog::TWEAKS.len());
        println!("{}", info.name);
        for tweak in &all {
            println!("{:<24} {:?}", tweak.id, tweak.state);
        }
        for app in apps(&packages, &store).iter().filter(|app| !app.packages.is_empty()) {
            println!("installed: {} {:?}", app.title, app.packages);
        }
        let classic = all.iter().find(|t| t.id == "classic-context-menu").unwrap();
        if !info.windows11 {
            assert_eq!(classic.state, State::Unavailable);
        }
        assert_eq!(apps(&packages, &store).len(), catalog::APPS.len());
    }
    #[test]
    fn turning_services_off_never_starts_what_windows_keeps_off() {
        let services = catalog::find("services").unwrap();
        let start_of = |name: &str| {
            let op = services.ops.iter().find(|op| matches!(op, Op::Service { name: n, .. } if *n == name)).unwrap();
            default_before(op)
        };
        assert_eq!(start_of("RemoteRegistry"), None);
        assert_eq!(start_of("RetailDemo"), Some(Before::Service { start: catalog::Start::Manual }));
        assert_eq!(start_of("TrkWks"), Some(Before::Service { start: catalog::Start::Auto }));
    }
    #[test]
    fn what_runs_as_administrator() {
        assert!(needs_admin(catalog::find("telemetry").unwrap()));
        assert!(needs_admin(catalog::find("edge").unwrap()));
        assert!(!needs_admin(catalog::find("clock-24h").unwrap()));
        assert!(!needs_admin(catalog::find("taskbar-search").unwrap()));
        assert!(!needs_admin(catalog::find("classic-context-menu").unwrap()));
        // The user's own policies, which only an administrator may write.
        assert!(catalog::user_policy(r"Software\Policies\Microsoft\Windows\WindowsAI"));
        assert!(catalog::user_policy(r"software\microsoft\windows\currentversion\policies\Explorer"));
        assert!(!catalog::user_policy(r"Software\Microsoft\Windows\CurrentVersion\Search"));
        assert!(!catalog::user_policy(r"Software\PoliciesAndMore\x"));
    }
    #[test]
    fn default_before_turns_every_op_off() {
        for tweak in catalog::TWEAKS {
            for op in tweak.ops {
                if matches!(op, Op::Appx { .. }) {
                    continue;
                }
                let Some(before) = default_before(op) else {
                    // Only a service Windows keeps off itself has nothing to go back to.
                    assert!(matches!(op, Op::Service { to, windows, .. } if to == windows), "{}: missing default_before for {op:?}", tweak.id);
                    continue;
                };
                match (*op, before) {
                    (Op::Reg { data, .. }, Before::Reg { value, .. }) => {
                        assert!(!value.matches(data), "{}: default_before {value:?} still matches {data:?}", tweak.id);
                    }
                    (Op::Service { to, from, windows, .. }, Before::Service { start }) => {
                        assert!(start == windows && start.rank() < to.rank() && from.contains(&start), "{}", tweak.id);
                    }
                    (Op::Task { .. }, Before::Task { enabled }) => assert!(enabled, "{}", tweak.id),
                    (Op::Clock24, Before::Clock { short, long }) => {
                        assert!(!system::is_24h(&short) && !system::is_24h(&long), "{}", tweak.id);
                    }
                    (Op::RemoveEdge, Before::Edge) | (Op::Capability { .. }, Before::Capability) => {}
                    _ => panic!("{}: mismatched default_before", tweak.id),
                }
            }
        }
    }
}
