// ! Everything the System Maintenance page can run, fixed at compile time. ! ! The page sends an action id and nothing else: no command, no arguments, no ! path. Programs are named by file name only and resolved under the real ! `%SystemRoot%\System32`, so nothing on `PATH` can stand in for them.
use std::path::PathBuf;
use serde::Serialize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    Network,
    Health,
    Software,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    No,
    /// Try without a prompt first; ask only if Windows refuses.
    IfNeeded,
    Yes,
}
pub struct Step {
    /// File name inside System32, never a path.
    pub exe: &'static str,
    /// The argument tail, passed through verbatim.
    pub args: &'static str,
    /// A non-zero exit here does not fail the action.
    pub allow_failure: bool,
}
pub enum Run {
    /// Runs the steps in order; when elevated, all of them share one UAC prompt.
    Batch(&'static [Step]),
    /// One long-running program whose output is streamed while it runs.
    Stream(Step),
    /// Windows Storage's online check, independent of the chkdsk executable.
    DiskScan,
    /// `winget upgrade --all`, which runs without elevation.
    WingetUpgradeAll,
}
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub run: Run,
    pub cancellable: bool,
    /// Shown by the page before starting; `None` runs straight away.
    pub confirm: Option<&'static str>,
    /// Shown by the page before cancelling a run in progress.
    pub cancel_confirm: Option<&'static str>,
    /// Success always means "restart to finish".
    pub reboot: bool,
}
pub struct Card {
    pub id: &'static str,
    pub section: Section,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub elevation: Elevation,
    pub caution: Option<&'static str>,
    /// Show the embedded live console under this card.
    pub console: bool,
    pub actions: &'static [Action],
}
impl Card {
    pub fn needs_admin(&self) -> bool {
        self.elevation == Elevation::Yes
    }
}
const NO_ACTION_EXTRAS: Action = Action {
    id: "",
    label: "",
    run: Run::Batch(&[]),
    cancellable: false,
    confirm: None,
    cancel_confirm: None,
    reboot: false,
};
pub const CARDS: &[Card] = &[
    // -- Network & connectivity ---------------------------------------------
    Card {
        id: "flush-dns",
        section: Section::Network,
        title: "Flush DNS Cache",
        description: "Clears the DNS resolver cache. Fixes sites that will not load because Windows remembers an old address.",
        icon: "dns",
        elevation: Elevation::IfNeeded,
        caution: None,
        console: false,
        actions: &[Action {
            id: "flush-dns",
            label: "Flush",
            run: Run::Batch(&[Step {
                exe: "ipconfig.exe",
                args: "/flushdns",
                allow_failure: false,
            }]),
            ..NO_ACTION_EXTRAS
        }],
    },
    Card {
        id: "ip-renew",
        section: Section::Network,
        title: "Release & Renew IP",
        description: "Drops the current local address and asks the router for a fresh one.",
        icon: "ip",
        elevation: Elevation::Yes,
        caution: None,
        console: false,
        actions: &[Action {
            id: "ip-renew",
            label: "Run",
            run: Run::Batch(&[
                Step {
                    exe: "ipconfig.exe",
                    args: "/release",
                    allow_failure: true,
                },
                Step {
                    exe: "ipconfig.exe",
                    args: "/renew",
                    allow_failure: false,
                },
            ]),
            confirm: Some(
                "Your network drops for a few seconds while Windows asks the router for a new address. Anything downloading right now will be interrupted.",
            ),
            ..NO_ACTION_EXTRAS
        }],
    },
    Card {
        id: "bluetooth-fix",
        section: Section::Network,
        title: "Fix Bluetooth",
        description: "Restarts the Bluetooth Support Service, which usually brings back devices that keep dropping out.",
        icon: "bluetooth",
        elevation: Elevation::Yes,
        caution: None,
        console: false,
        actions: &[Action {
            id: "bluetooth-fix",
            label: "Fix",
            run: Run::Batch(&[
                Step {
                    exe: "net.exe",
                    args: "stop bthserv /y",
                    allow_failure: true,
                },
                Step {
                    exe: "net.exe",
                    args: "start bthserv",
                    allow_failure: false,
                },
                Step {
                    exe: "net.exe",
                    args: "start BTAGService",
                    allow_failure: true,
                },
            ]),
            ..NO_ACTION_EXTRAS
        }],
    },
    Card {
        id: "net-reset",
        section: Section::Network,
        title: "Network Reset",
        description: "Resets Winsock and the IP stack to their defaults. The last resort when nothing else fixes connectivity.",
        icon: "network",
        elevation: Elevation::Yes,
        caution: Some("Removes third-party network drivers and needs a restart"),
        console: false,
        actions: &[Action {
            id: "net-reset",
            label: "Reset",
            run: Run::Batch(&[
                Step {
                    exe: "netsh.exe",
                    args: "winsock reset",
                    allow_failure: false,
                },
                Step {
                    exe: "netsh.exe",
                    args: "int ip reset",
                    allow_failure: false,
                },
                Step {
                    exe: "netsh.exe",
                    args: "int ipv6 reset",
                    allow_failure: true,
                },
            ]),
            confirm: Some(
                "This resets Winsock and the IP stack. VPN and antivirus network drivers are removed and may need reinstalling, and you must restart the PC afterwards.",
            ),
            reboot: true,
            ..NO_ACTION_EXTRAS
        }],
    },
    // -- System health & diagnostics ----------------------------------------
    Card {
        id: "system-repair",
        section: Section::Health,
        title: "System File & Image Repair",
        description: "SFC checks and repairs Windows' own files. DISM repairs the component store SFC restores from, so run it if SFC cannot fix everything.",
        icon: "repair",
        elevation: Elevation::Yes,
        caution: None,
        console: true,
        actions: &[
            Action {
                id: "sfc",
                label: "Run SFC",
                run: Run::Stream(Step {
                    exe: "sfc.exe",
                    args: "/scannow",
                    allow_failure: false,
                }),
                cancellable: true,
                ..NO_ACTION_EXTRAS
            },
            Action {
                id: "dism",
                label: "Run DISM",
                run: Run::Stream(Step {
                    exe: "Dism.exe",
                    args: "/Online /Cleanup-Image /RestoreHealth",
                    allow_failure: false,
                }),
                cancellable: true,
                cancel_confirm: Some(
                    "Stopping DISM part-way can leave pending actions in the component store. You should run it again afterwards.",
                ),
                ..NO_ACTION_EXTRAS
            },
        ],
    },
    Card {
        id: "chkdsk",
        section: Section::Health,
        title: "Check Disk",
        description: "Checks the Windows system drive online and reports file-system problems. No offline repair or restart is requested. Let the scan finish once started.",
        icon: "disk",
        elevation: Elevation::Yes,
        caution: None,
        console: true,
        actions: &[Action {
            id: "chkdsk",
            label: "Check",
            run: Run::DiskScan,
            // The Storage service owns the scan: killing its PowerShell client does not reliably cancel it, so retain the task lock.
            ..NO_ACTION_EXTRAS
        }],
    },
    Card {
        id: "audio-restart",
        section: Section::Health,
        title: "Restart Audio System",
        description: "Restarts the Windows audio services. Brings sound back without rebooting.",
        icon: "audio",
        elevation: Elevation::Yes,
        caution: None,
        console: false,
        actions: &[Action {
            id: "audio-restart",
            label: "Restart",
            run: Run::Batch(&[
                Step {
                    exe: "net.exe",
                    args: "stop AudioEndpointBuilder /y",
                    allow_failure: true,
                },
                Step {
                    exe: "net.exe",
                    args: "start AudioEndpointBuilder",
                    allow_failure: false,
                },
                Step {
                    exe: "net.exe",
                    args: "start Audiosrv",
                    allow_failure: true,
                },
            ]),
            confirm: Some(
                "Sound cuts out for a second or two. Apps that are playing audio may need restarting.",
            ),
            ..NO_ACTION_EXTRAS
        }],
    },
    // -- Software updates ---------------------------------------------------
    Card {
        id: "update-apps",
        section: Section::Software,
        title: "Update All Applications",
        description: "Upgrades every installed program winget knows about, including ones whose current version it cannot read.",
        icon: "updates",
        elevation: Elevation::No,
        caution: None,
        console: true,
        actions: &[Action {
            id: "update-apps",
            label: "Update all",
            run: Run::WingetUpgradeAll,
            cancellable: true,
            confirm: Some(
                "Windows may ask for administrator approval once per program, and this can take a while.",
            ),
            ..NO_ACTION_EXTRAS
        }],
    },
];
pub fn find(id: &str) -> Option<(&'static Card, &'static Action)> {
    CARDS
        .iter()
        .flat_map(|card| card.actions.iter().map(move |action| (card, action)))
        .find(|(_, action)| action.id == id)
}
/// `%SystemRoot%\System32\<exe>`, never resolved through `PATH`.
pub fn system32(exe: &str) -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    // A 32-bit build would be redirected to SysWOW64, where sfc and chkdsk do not exist; `Sysnative` would be the escape hatch. We only ship x64.
    PathBuf::from(root).join("System32").join(exe)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn steps(action: &Action) -> Vec<&Step> {
        match &action.run {
            Run::Batch(list) => list.iter().collect(),
            Run::Stream(step) => vec![step],
            Run::DiskScan | Run::WingetUpgradeAll => Vec::new(),
        }
    }
    fn actions() -> impl Iterator<Item = (&'static Card, &'static Action)> {
        CARDS
            .iter()
            .flat_map(|card| card.actions.iter().map(move |action| (card, action)))
    }
    fn all_distinct(ids: &[&str]) -> bool {
        let mut unique = ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        unique.len() == ids.len()
    }
    #[test]
    fn every_card_and_action_is_described_and_uniquely_named() {
        for card in CARDS {
            assert!(
                !card.title.is_empty() && !card.description.is_empty(),
                "{}",
                card.id
            );
            assert!(!card.icon.is_empty(), "{}", card.id);
            assert!(!card.actions.is_empty(), "{}", card.id);
        }
        for (_, action) in actions() {
            assert!(!action.label.is_empty(), "{}", action.id);
        }
        // A one-action card shares its id with that action on purpose; what must not repeat is an id within either list.
        assert!(all_distinct(
            &CARDS.iter().map(|c| c.id).collect::<Vec<_>>()
        ));
        assert!(all_distinct(
            &actions().map(|(_, a)| a.id).collect::<Vec<_>>()
        ));
    }
    /// The reason the argument tails can be passed through verbatim: nothing in
    /// the table can end a quoted string or start a new command.
    #[test]
    fn no_argument_can_escape_its_quoting() {
        let safe = |text: &str| {
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || " /:\\.,=+_-".contains(c))
        };
        for (_, action) in actions() {
            for step in steps(action) {
                assert!(
                    safe(step.args),
                    "unsafe args for {}: {}",
                    action.id,
                    step.args
                );
                assert!(
                    step.exe.ends_with(".exe")
                        && safe(step.exe)
                        && !step.exe.contains(['\\', '/', ' ']),
                    "{} is not a plain System32 file name",
                    step.exe
                );
            }
        }
    }
    #[test]
    fn every_program_exists_on_this_machine() {
        for (_, action) in actions() {
            for step in steps(action) {
                let path = system32(step.exe);
                assert!(path.is_file(), "missing: {}", path.display());
            }
        }
    }
    /// The Storage scan must keep its lane until the service has returned.
    #[test]
    fn check_disk_uses_the_online_scan_and_does_not_offer_false_cancellation() {
        let (_, action) = find("chkdsk").unwrap();
        assert!(matches!(action.run, Run::DiskScan));
        assert!(!action.cancellable);
        assert!(!action.reboot);
    }
    #[test]
    fn only_streaming_actions_get_a_console() {
        for card in CARDS.iter().filter(|c| c.console) {
            for action in card.actions {
                assert!(
                    matches!(action.run, Run::Stream(_) | Run::DiskScan | Run::WingetUpgradeAll),
                    "{} has a console but produces no live output",
                    action.id
                );
            }
        }
        for (_, action) in actions().filter(|(_, a)| a.cancellable) {
            assert!(
                matches!(action.run, Run::Stream(_) | Run::WingetUpgradeAll),
                "{} claims to be cancellable but runs as one batch",
                action.id
            );
        }
    }
    #[test]
    fn the_disruptive_actions_ask_first() {
        for id in ["net-reset", "ip-renew", "audio-restart", "update-apps"] {
            assert!(
                find(id).unwrap().1.confirm.is_some(),
                "{id} must confirm first"
            );
        }
        assert!(find("dism").unwrap().1.cancel_confirm.is_some());
        assert!(
            find("flush-dns").unwrap().1.confirm.is_none(),
            "harmless, no dialog"
        );
    }
    #[test]
    fn unknown_ids_are_dropped_rather_than_guessed() {
        assert!(find("nope").is_none());
        assert!(find(r"..\..\Windows").is_none());
        assert!(find("").is_none(), "the empty id must not match");
    }
    #[test]
    fn network_reset_is_the_only_one_that_always_needs_a_restart() {
        let reboot: Vec<&str> = actions()
            .filter(|(_, a)| a.reboot)
            .map(|(_, a)| a.id)
            .collect();
        assert_eq!(reboot, ["net-reset"]);
    }
}
