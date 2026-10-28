// ! The debloater's fixed tables: every tweak and every Store app it can ! touch. The page sends only ids from here, and the administrator helper ! acts only on what these tables name, never on paths or commands it is ! sent. ! ! Values come from what Chris Titus's WinUtil, Raphire's Win11Debloat and ! Sparkle do, checked against Microsoft's documentation, with their known ! mistakes left out (see `docs/DEVELOPMENT.md`).
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Privacy,
    Taskbar,
    Explorer,
    Ai,
    System,
    Apps,
    /// Parts of Windows itself (Features on Demand), removed whole.
    Features,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Safe,
    /// Harder to take back, or changes more than it seems.
    Caution,
}
/// The Quick setup profile a tweak or an app first belongs to. A profile
/// takes in everything up to its own level: Recommended includes Light.
/// `None` (outside every profile) is for personal taste and for what is
/// harder to take back: those are only ever done when picked one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Privacy and ads: nothing looks or works differently.
    Light,
    /// What nearly everyone is better off with.
    Recommended,
    /// Everything else that makes Windows leaner, with small trade-offs.
    Maximum,
}
pub const LIGHT: Option<Level> = Some(Level::Light);
pub const RECOMMENDED: Option<Level> = Some(Level::Recommended);
pub const MAXIMUM: Option<Level> = Some(Level::Maximum);
/// Never part of a profile: only when the user picks it.
pub const OPT_IN: Option<Level> = None;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    /// HKEY_CURRENT_USER: written by the app itself, as the user.
    User,
    /// HKEY_LOCAL_MACHINE: written by the administrator helper.
    Machine,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Data {
    Dword(u32),
    Sz(&'static str),
}
/// A service's start type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Start {
    Auto,
    AutoDelayed,
    Manual,
    Disabled,
}
impl Start {
    /// How far the service is from running by itself: Disabled is the most.
    pub fn rank(self) -> u8 {
        match self {
            Start::Auto => 0,
            Start::AutoDelayed => 1,
            Start::Manual => 2,
            Start::Disabled => 3,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// A registry value. `name` "" is the key's default value.
    Reg {
        hive: Hive,
        path: &'static str,
        name: &'static str,
        data: Data,
        /// Windows may refuse it (UserChoice protection); the rest goes on.
        best_effort: bool,
    },
    /// A service's start type, changed only from one of `from` (what Windows
    /// may have), so a service the user set up by hand is left alone.
    Service {
        name: &'static str,
        to: Start,
        from: &'static [Start],
        /// The start type Windows ships with: what turning the tweak off
        /// puts back when MYLE kept nothing. Nothing when it is `to` already.
        windows: Start,
    },
    /// A scheduled task, disabled: `folder` and `name` as Task Scheduler
    /// shows them.
    Task {
        folder: &'static str,
        name: &'static str,
    },
    /// A Store app removed for every user, and for users still to come.
    Appx { app: &'static str },
    /// The 24-hour clock, for the user's own format.
    Clock24,
    /// Microsoft Edge removed (WebView2 stays).
    RemoveEdge,
    /// A part of Windows (a Feature on Demand) removed: `name` is the start of
    /// its capability name ("App.StepsRecorder~~~~"), `package` the start of
    /// its servicing package's, which tells without administrator rights
    /// whether it is installed.
    Capability { name: &'static str, package: &'static str },
}
impl Op {
    /// Whether the app changes it itself, as the user; the rest is the
    /// administrator helper's, the user's own policies included.
    pub fn as_user(&self) -> bool {
        match self {
            Op::Reg { hive: Hive::User, path, .. } => !user_policy(path),
            Op::Clock24 => true,
            _ => false,
        }
    }
}
/// Whether a key of the user's own hive holds policies: Windows lets the
/// user read them but only an administrator write them.
pub fn user_policy(path: &str) -> bool {
    [r"Software\Policies\", r"Software\Microsoft\Windows\CurrentVersion\Policies\"]
        .iter()
        .any(|policies| path.get(..policies.len()).is_some_and(|start| start.eq_ignore_ascii_case(policies)))
}
/// Windows builds a tweak is for: `min` inclusive, `max` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builds {
    pub min: u32,
    pub max: u32,
}
pub const ANY: Builds = Builds { min: 0, max: u32::MAX };
pub const WIN11: Builds = Builds { min: 22000, max: u32::MAX };
/// Windows 11 23H2 and later.
pub const WIN11_23H2: Builds = Builds { min: 22631, max: u32::MAX };
impl Builds {
    pub fn contains(self, build: u32) -> bool {
        build >= self.min && build < self.max
    }
}
#[derive(Debug)]
pub struct Tweak {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub category: Category,
    pub risk: Risk,
    pub builds: Builds,
    /// The Quick setup profile it is part of.
    pub level: Option<Level>,
    /// What to know before turning it on, in plain words: what stops working
    /// or works differently. Shown next to the summary.
    pub note: Option<&'static str>,
    /// Explorer is restarted afterwards so it shows at once.
    pub restart_explorer: bool,
    /// Complete only after the PC restarts.
    pub reboot: bool,
    /// Asked again on its own before it runs.
    pub confirm: Option<&'static str>,
    pub ops: &'static [Op],
}
const fn user(path: &'static str, name: &'static str, data: Data) -> Op {
    Op::Reg { hive: Hive::User, path, name, data, best_effort: false }
}
const fn machine(path: &'static str, name: &'static str, data: Data) -> Op {
    Op::Reg { hive: Hive::Machine, path, name, data, best_effort: false }
}
const fn user_best_effort(path: &'static str, name: &'static str, data: Data) -> Op {
    Op::Reg { hive: Hive::User, path, name, data, best_effort: true }
}
const fn task(folder: &'static str, name: &'static str) -> Op {
    Op::Task { folder, name }
}
use Data::{Dword, Sz};
const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const SEARCH: &str = r"Software\Microsoft\Windows\CurrentVersion\Search";
const APP_EXPERIENCE: &str = r"\Microsoft\Windows\Application Experience";
const CEIP: &str = r"\Microsoft\Windows\Customer Experience Improvement Program";
const CONTENT_DELIVERY: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const AUTO_START: &[Start] = &[Start::Auto, Start::AutoDelayed];
const NOT_DISABLED: &[Start] = &[Start::Auto, Start::AutoDelayed, Start::Manual];
pub const TWEAKS: &[Tweak] = &[
    Tweak {
        id: "telemetry",
        title: "Turn off telemetry",
        summary: "Windows sends Microsoft as little about how you use the PC as it allows, keeps no advertising ID and stops asking for feedback.",
        category: Category::Privacy,
        risk: Risk::Safe,
        builds: ANY,
        level: LIGHT,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            user(r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", Dword(0)),
            user(r"Software\Microsoft\Windows\CurrentVersion\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled", Dword(0)),
            user(r"Software\Microsoft\Speech_OneCore\Settings\OnlineSpeechPrivacy", "HasAccepted", Dword(0)),
            user(r"Software\Microsoft\Input\TIPC", "Enabled", Dword(0)),
            user(r"Software\Microsoft\InputPersonalization", "RestrictImplicitInkCollection", Dword(1)),
            user(r"Software\Microsoft\InputPersonalization", "RestrictImplicitTextCollection", Dword(1)),
            user(r"Software\Microsoft\InputPersonalization\TrainedDataStore", "HarvestContacts", Dword(0)),
            user(r"Software\Microsoft\Personalization\Settings", "AcceptedPrivacyPolicy", Dword(0)),
            user(r"Software\Microsoft\Siuf\Rules", "NumberOfSIUFInPeriod", Dword(0)),
            user(ADVANCED, "Start_TrackProgs", Dword(0)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", "AllowTelemetry", Dword(0)),
            machine(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection", "AllowTelemetry", Dword(0)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", "DoNotShowFeedbackNotifications", Dword(1)),
            Op::Service { name: "DiagTrack", to: Start::Disabled, from: NOT_DISABLED, windows: Start::Auto },
            task(APP_EXPERIENCE, "Microsoft Compatibility Appraiser"),
            task(APP_EXPERIENCE, "Microsoft Compatibility Appraiser Exp"),
            task(APP_EXPERIENCE, "ProgramDataUpdater"),
            task(APP_EXPERIENCE, "StartupAppTask"),
            task(CEIP, "Consolidator"),
            task(CEIP, "UsbCeip"),
            task(r"\Microsoft\Windows\DiskDiagnostic", "Microsoft-Windows-DiskDiagnosticDataCollector"),
            task(r"\Microsoft\Windows\Autochk", "Proxy"),
        ],
    },
    Tweak {
        id: "edge",
        title: "Remove Microsoft Edge",
        summary: "Uninstalls the Edge browser. WebView2, which MYLE and other apps are built on, stays. Edge profiles that are not synced are kept on disk.",
        category: Category::Apps,
        risk: Risk::Caution,
        builds: ANY,
        level: OPT_IN,
        note: Some("Links that open in Edge will ask for another browser. Edge can be installed again with Undo."),
        restart_explorer: false,
        reboot: false,
        confirm: Some("Remove Microsoft Edge? Links that open in Edge by default will ask for another browser. It can be installed again with Undo, or from microsoft.com/edge."),
        ops: &[Op::RemoveEdge],
    },
    Tweak {
        id: "clock-24h",
        title: "24-hour clock",
        summary: "The taskbar clock and every app show the time as 13:45 instead of 1:45 PM.",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: Some("Changes the time format of your user account, not only the taskbar."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Clock24],
    },
    Tweak {
        id: "background-apps",
        title: "Stop Store apps running in the background",
        summary: "Store apps no longer run, update tiles or use the network while they are closed.",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: RECOMMENDED,
        note: Some("Store apps such as Mail update and notify only while they are open."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            user(r"Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications", "GlobalUserDisabled", Dword(1)),
            user(SEARCH, "BackgroundAppGlobalToggle", Dword(0)),
        ],
    },
    Tweak {
        id: "services",
        title: "Set unneeded services to manual",
        summary: "Background services most people never use (Maps, Retail Demo, Remote Registry and a few more) start only when something needs them. Services you set yourself are left alone.",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: RECOMMENDED,
        note: Some("Takes full effect after the next restart."),
        restart_explorer: false,
        reboot: true,
        confirm: None,
        ops: &[
            Op::Service { name: "MapsBroker", to: Start::Manual, from: AUTO_START, windows: Start::AutoDelayed },
            Op::Service { name: "PcaSvc", to: Start::Manual, from: AUTO_START, windows: Start::Auto },
            Op::Service { name: "TrkWks", to: Start::Manual, from: AUTO_START, windows: Start::Auto },
            Op::Service { name: "StorSvc", to: Start::Manual, from: AUTO_START, windows: Start::Auto },
            Op::Service { name: "RetailDemo", to: Start::Disabled, from: NOT_DISABLED, windows: Start::Manual },
            Op::Service { name: "RemoteRegistry", to: Start::Disabled, from: NOT_DISABLED, windows: Start::Disabled },
        ],
    },
    Tweak {
        id: "copilot",
        title: "Turn off Copilot",
        summary: "Removes the Copilot app and button, and turns off Recall, Click to Do and the AI features in Notepad.",
        category: Category::Ai,
        risk: Risk::Safe,
        builds: ANY,
        level: RECOMMENDED,
        note: Some("Copilot, Recall and Click to Do go away until you turn this off again."),
        restart_explorer: true,
        reboot: false,
        confirm: None,
        ops: &[
            user(r"Software\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", Dword(1)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", Dword(1)),
            user(ADVANCED, "ShowCopilotButton", Dword(0)),
            Op::Appx { app: "Microsoft.Copilot" },
            user(r"Software\Policies\Microsoft\Windows\WindowsAI", "DisableAIDataAnalysis", Dword(1)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI", "DisableAIDataAnalysis", Dword(1)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI", "AllowRecallEnablement", Dword(0)),
            user(r"Software\Policies\Microsoft\Windows\WindowsAI", "DisableClickToDo", Dword(1)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI", "DisableClickToDo", Dword(1)),
            machine(r"SOFTWARE\Policies\WindowsNotepad", "DisableAIFeatures", Dword(1)),
            Op::Service { name: "WSAIFabricSvc", to: Start::Manual, from: AUTO_START, windows: Start::Auto },
        ],
    },
    Tweak {
        id: "location",
        title: "Turn off location tracking",
        summary: "Apps and Windows can no longer ask where the PC is, and offline maps stop updating.",
        category: Category::Privacy,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: Some("Maps, weather and Find my device can no longer tell where the PC is."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            Op::Service { name: "lfsvc", to: Start::Disabled, from: NOT_DISABLED, windows: Start::Manual },
            machine(r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location", "Value", Sz("Deny")),
            user(r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location", "Value", Sz("Deny")),
            machine(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Sensor\Overrides\{BFA794E4-F964-4FDB-90F6-51056BFE4B44}", "SensorPermissionState", Dword(0)),
            machine(r"SYSTEM\Maps", "AutoUpdateEnabled", Dword(0)),
        ],
    },
    Tweak {
        id: "taskbar-search",
        title: "Hide search on the taskbar",
        summary: "Removes the search box or icon; Start still searches when you type.",
        category: Category::Taskbar,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: Some("Press the Windows key and type: Start still searches."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[user(SEARCH, "SearchboxTaskbarMode", Dword(0))],
    },
    Tweak {
        id: "end-task",
        title: "End Task on right-click",
        summary: "Right-click an app on the taskbar to close it at once, as Task Manager would.",
        category: Category::Taskbar,
        risk: Risk::Safe,
        builds: WIN11_23H2,
        level: RECOMMENDED,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[user(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\TaskbarDeveloperSettings", "TaskbarEndTask", Dword(1))],
    },
    Tweak {
        id: "taskview-widgets",
        title: "Hide Task View and Widgets",
        summary: "Removes the Task View button and the Widgets (weather and news) board from the taskbar.",
        category: Category::Taskbar,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: Some("Win+Tab still opens Task View; the Widgets board stays reachable with Win+W."),
        restart_explorer: true,
        reboot: false,
        confirm: None,
        ops: &[
            user(ADVANCED, "ShowTaskViewButton", Dword(0)),
            // Windows 11 protects TaskbarDa; the policy works regardless.
            user_best_effort(ADVANCED, "TaskbarDa", Dword(0)),
            machine(r"SOFTWARE\Policies\Microsoft\Dsh", "AllowNewsAndInterests", Dword(0)),
            // Windows 10's "News and interests": Windows 11 24H2 refuses it (access denied); the policy below works regardless.
            user_best_effort(r"Software\Microsoft\Windows\CurrentVersion\Feeds", "ShellFeedsTaskbarViewMode", Dword(2)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\Windows Feeds", "EnableFeeds", Dword(0)),
        ],
    },
    Tweak {
        id: "bing",
        title: "Remove Bing from search",
        summary: "Start searches only your PC: no web results, no Bing suggestions, and the Bing Search app is removed.",
        category: Category::Privacy,
        risk: Risk::Safe,
        builds: ANY,
        level: RECOMMENDED,
        note: None,
        restart_explorer: true,
        reboot: false,
        confirm: None,
        ops: &[
            user(SEARCH, "BingSearchEnabled", Dword(0)),
            user(SEARCH, "CortanaConsent", Dword(0)),
            user(r"Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions", Dword(1)),
            Op::Appx { app: "Microsoft.BingSearch" },
        ],
    },
    Tweak {
        id: "classic-context-menu",
        title: "Classic right-click menu",
        summary: "The full Windows 10 right-click menu in File Explorer, without \"Show more options\".",
        category: Category::Explorer,
        risk: Risk::Safe,
        builds: WIN11,
        level: MAXIMUM,
        note: None,
        restart_explorer: true,
        reboot: false,
        confirm: None,
        ops: &[user(r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32", "", Sz(""))],
    },
    Tweak {
        id: "suggestions",
        title: "Stop suggested apps and ads",
        summary: "Windows no longer installs promoted apps (Candy Crush and the like) by itself, nor shows suggestions in Start, Settings and tips.",
        category: Category::Privacy,
        risk: Risk::Safe,
        builds: ANY,
        level: LIGHT,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            user(CONTENT_DELIVERY, "SilentInstalledAppsEnabled", Dword(0)),
            user(CONTENT_DELIVERY, "PreInstalledAppsEnabled", Dword(0)),
            user(CONTENT_DELIVERY, "OemPreInstalledAppsEnabled", Dword(0)),
            user(CONTENT_DELIVERY, "SystemPaneSuggestionsEnabled", Dword(0)),
            user(CONTENT_DELIVERY, "SoftLandingEnabled", Dword(0)),
            user(CONTENT_DELIVERY, "SubscribedContent-338388Enabled", Dword(0)),
            user(CONTENT_DELIVERY, "SubscribedContent-338389Enabled", Dword(0)),
            user(CONTENT_DELIVERY, "SubscribedContent-353694Enabled", Dword(0)),
            user(CONTENT_DELIVERY, "SubscribedContent-353696Enabled", Dword(0)),
            user(ADVANCED, "Start_IrisRecommendations", Dword(0)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\CloudContent", "DisableWindowsConsumerFeatures", Dword(1)),
        ],
    },
    // Not part of the one-click Debloat: offered one by one.
    Tweak {
        id: "activity-history",
        title: "Turn off activity history",
        summary: "Windows stops recording which apps and files you use. Clipboard history keeps working.",
        category: Category::Privacy,
        risk: Risk::Safe,
        builds: ANY,
        level: LIGHT,
        note: Some("Windows stops suggesting recent files and apps based on what you did."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            machine(r"SOFTWARE\Policies\Microsoft\Windows\System", "PublishUserActivities", Dword(0)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\System", "UploadUserActivities", Dword(0)),
        ],
    },
    Tweak {
        id: "delivery-optimization",
        title: "No update sharing with other PCs",
        summary: "Windows Update downloads only from Microsoft, never from or to other PCs.",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: LIGHT,
        note: Some("Updates may download a little slower when several PCs share a network."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[machine(r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization", "DODownloadMode", Dword(0))],
    },
    Tweak {
        id: "game-dvr",
        title: "Turn off Game DVR",
        summary: "No background recording of games by the Xbox Game Bar.",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: Some("Xbox Game Bar can no longer save the last minutes of a game."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            user(r"System\GameConfigStore", "GameDVR_Enabled", Dword(0)),
            machine(r"SOFTWARE\Policies\Microsoft\Windows\GameDVR", "AllowGameDVR", Dword(0)),
        ],
    },
    Tweak {
        id: "file-extensions",
        title: "Show file extensions",
        summary: "File Explorer shows .exe, .pdf and the rest, so a file cannot pose as another kind.",
        category: Category::Explorer,
        risk: Risk::Safe,
        builds: ANY,
        level: RECOMMENDED,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[user(ADVANCED, "HideFileExt", Dword(0))],
    },
    Tweak {
        id: "hidden-files",
        title: "Show hidden files",
        summary: "File Explorer shows hidden files and folders, such as AppData.",
        category: Category::Explorer,
        risk: Risk::Safe,
        builds: ANY,
        level: OPT_IN,
        note: Some("System files show too: leave alone what you do not know."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[user(ADVANCED, "Hidden", Dword(1))],
    },
    Tweak {
        id: "dark-mode",
        title: "Dark mode",
        summary: "Windows and apps use the dark theme.",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: OPT_IN,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            user(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme", Dword(0)),
            user(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "SystemUsesLightTheme", Dword(0)),
        ],
    },
    Tweak {
        id: "taskbar-left",
        title: "Taskbar icons on the left",
        summary: "Start and the taskbar icons sit on the left, as in Windows 10.",
        category: Category::Taskbar,
        risk: Risk::Safe,
        builds: WIN11,
        level: OPT_IN,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[user(ADVANCED, "TaskbarAl", Dword(0))],
    },
    Tweak {
        id: "mouse-acceleration",
        title: "Turn off mouse acceleration",
        summary: "The pointer moves as far as the mouse does, however fast: steadier aim in games.",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: OPT_IN,
        note: Some("Feels different at first if you are used to acceleration."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[
            user(r"Control Panel\Mouse", "MouseSpeed", Sz("0")),
            user(r"Control Panel\Mouse", "MouseThreshold1", Sz("0")),
            user(r"Control Panel\Mouse", "MouseThreshold2", Sz("0")),
        ],
    },
    Tweak {
        id: "sticky-keys",
        title: "No Sticky Keys prompt",
        summary: "Pressing Shift five times no longer asks about Sticky Keys (handy in games).",
        category: Category::System,
        risk: Risk::Safe,
        builds: ANY,
        level: OPT_IN,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[user(r"Control Panel\Accessibility\StickyKeys", "Flags", Sz("506"))],
    },
    Tweak {
        id: "steps-recorder",
        title: "Remove Steps Recorder",
        summary: "Records the steps of a problem as screenshots and text. Microsoft is retiring it.",
        category: Category::Features,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "App.StepsRecorder~~~~", package: "Microsoft-Windows-StepsRecorder-Package~" }],
    },
    Tweak {
        id: "math-input",
        title: "Remove Math Input Panel",
        summary: "Turns handwritten maths into text for documents.",
        category: Category::Features,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "MathRecognizer~~~~", package: "Microsoft-Windows-TabletPCMath-Package~" }],
    },
    Tweak {
        id: "wordpad",
        title: "Remove WordPad",
        summary: "The old rich-text editor, which Microsoft has retired.",
        category: Category::Features,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "Microsoft.Windows.WordPad~~~~", package: "Microsoft-Windows-WordPad-FoD-Package~" }],
    },
    Tweak {
        id: "xps-viewer",
        title: "Remove XPS Viewer",
        summary: "Opens .xps documents, a format few people use.",
        category: Category::Features,
        risk: Risk::Safe,
        builds: ANY,
        level: MAXIMUM,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "XPS.Viewer~~~~", package: "Microsoft-Windows-Xps-Xps-Viewer-Opt-Package~" }],
    },
    Tweak {
        id: "media-player-legacy",
        title: "Remove Windows Media Player Legacy",
        summary: "The classic Windows Media Player. The new Media Player stays.",
        category: Category::Features,
        risk: Risk::Safe,
        builds: ANY,
        level: OPT_IN,
        note: Some("Some old programs, DVDs and audio CDs play only in it."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "Media.WindowsMediaPlayer~~~~", package: "Microsoft-Windows-MediaPlayer-Package~" }],
    },
    Tweak {
        id: "powershell-ise",
        title: "Remove PowerShell ISE",
        summary: "The old script editor for PowerShell. PowerShell itself stays.",
        category: Category::Features,
        risk: Risk::Safe,
        builds: ANY,
        level: OPT_IN,
        note: None,
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "Microsoft.Windows.PowerShell.ISE~~~~", package: "Microsoft-Windows-PowerShell-ISE-FOD-Package~" }],
    },
    Tweak {
        id: "ie-mode",
        title: "Remove Internet Explorer mode",
        summary: "The old engine behind Internet Explorer mode in Edge.",
        category: Category::Features,
        risk: Risk::Caution,
        builds: ANY,
        level: OPT_IN,
        note: Some("Old company and government sites that need Internet Explorer mode stop working in Edge."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "Browser.InternetExplorer~~~~", package: "Microsoft-Windows-InternetExplorer-Optional-Package~" }],
    },
    Tweak {
        id: "fax-scan",
        title: "Remove Windows Fax and Scan",
        summary: "Sends faxes and scans documents from a scanner.",
        category: Category::Features,
        risk: Risk::Safe,
        builds: ANY,
        level: OPT_IN,
        note: Some("Keep it if you scan with it."),
        restart_explorer: false,
        reboot: false,
        confirm: None,
        ops: &[Op::Capability { name: "Print.Fax.Scan~~~~", package: "Microsoft-Windows-Fax-Common-FoD-Package~" }],
    },
];
pub fn find(id: &str) -> Option<&'static Tweak> {
    TWEAKS.iter().find(|tweak| tweak.id == id)
}
/// Values the administrator helper may write back on undo for a string it
/// changed: an undo record is the app's file, so it could have been edited.
pub fn allowed_helper_strings(path: &str, name: &str) -> &'static [&'static str] {
    if path.ends_with(r"ConsentStore\location") && name == "Value" {
        &["Allow", "Deny", "Prompt"]
    } else {
        &[]
    }
}
// --------------------------------------------------------------------------- Store apps
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppGroup {
    Microsoft,
    Bing,
    Xbox,
    ThirdParty,
}
/// How an app's package name is recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Name {
    Exact(&'static str),
    /// Third-party packages carry a publisher prefix: `king.com.CandyCrushSaga`
    /// is `Suffix(".CandyCrushSaga")`.
    Suffix(&'static str),
    /// For the few whose package name changes between releases.
    Contains(&'static str),
}
impl Name {
    pub fn matches(self, package: &str) -> bool {
        match self {
            Name::Exact(exact) => package.eq_ignore_ascii_case(exact),
            Name::Suffix(suffix) => {
                package.len() > suffix.len()
                    && package.is_char_boundary(package.len() - suffix.len())
                    && package[package.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
            }
            Name::Contains(part) => package.to_ascii_lowercase().contains(&part.to_ascii_lowercase()),
        }
    }
}
#[derive(Debug)]
pub struct App {
    /// Stable id for the page.
    pub id: &'static str,
    pub title: &'static str,
    pub name: Name,
    pub group: AppGroup,
    /// The Quick setup profile that removes it.
    pub level: Option<Level>,
    /// Its Microsoft Store product id, to install it again.
    pub store_id: Option<&'static str>,
    /// What it is, in a few words, for someone who does not know.
    pub about: &'static str,
    /// Why someone might keep it, when there is a reason.
    pub keep: Option<&'static str>,
}
impl App {
    const fn about(self, about: &'static str) -> App {
        App { about, ..self }
    }
    const fn keep(self, keep: &'static str) -> App {
        App { keep: Some(keep), ..self }
    }
}
const fn app(id: &'static str, title: &'static str, name: &'static str, group: AppGroup, level: Option<Level>, store_id: Option<&'static str>) -> App {
    App { id, title, name: Name::Exact(name), group, level, store_id, about: "", keep: None }
}
const fn promo(id: &'static str, title: &'static str, suffix: &'static str, store_id: Option<&'static str>) -> App {
    App { id, title, name: Name::Suffix(suffix), group: AppGroup::ThirdParty, level: LIGHT, store_id, about: "", keep: None }
}
use AppGroup::{Bing, Microsoft, Xbox};
pub const APPS: &[App] = &[
    // Microsoft, removed by the one-click Debloat.
    app("clipchamp", "Clipchamp", "Clipchamp.Clipchamp", Microsoft, RECOMMENDED, Some("9P1J8S7CCWWT")).about("Video editor."),
    app("cortana", "Cortana", "Microsoft.549981C3F5F10", Microsoft, RECOMMENDED, None).about("The old voice assistant; it no longer does anything."),
    app("dev-home", "Dev Home", "Microsoft.Windows.DevHome", Microsoft, RECOMMENDED, None).about("Dashboard for programmers; Microsoft is retiring it."),
    app("feedback-hub", "Feedback Hub", "Microsoft.WindowsFeedbackHub", Microsoft, RECOMMENDED, Some("9NBLGGH4R32N")).about("Sends feedback and bug reports to Microsoft."),
    app("get-started", "Get Started (Tips)", "Microsoft.Getstarted", Microsoft, RECOMMENDED, None).about("Tips for new Windows users."),
    app("office-hub", "Microsoft 365 (Office)", "Microsoft.MicrosoftOfficeHub", Microsoft, RECOMMENDED, Some("9WZDNCRD29V9")).about("Shortcut to Microsoft 365; Word and Excel stay if installed."),
    app("solitaire", "Solitaire Collection", "Microsoft.MicrosoftSolitaireCollection", Microsoft, RECOMMENDED, Some("9WZDNCRFHWD2")).about("Card games, with ads."),
    app("power-automate", "Power Automate", "Microsoft.PowerAutomateDesktop", Microsoft, RECOMMENDED, None).about("Records and runs automations; for advanced users."),
    app("todo", "Microsoft To Do", "Microsoft.Todos", Microsoft, RECOMMENDED, Some("9NBLGGH5R558")).about("To-do lists, synced with a Microsoft account.").keep("Keep it if you keep your lists in it."),
    app("news", "Microsoft News", "Microsoft.News", Microsoft, RECOMMENDED, None).about("News headlines from Microsoft."),
    app("3d-builder", "3D Builder", "Microsoft.3DBuilder", Microsoft, RECOMMENDED, None).about("Makes 3D models; discontinued."),
    app("3d-viewer", "3D Viewer", "Microsoft.Microsoft3DViewer", Microsoft, RECOMMENDED, None).about("Views 3D models; discontinued."),
    app("print-3d", "Print 3D", "Microsoft.Print3D", Microsoft, RECOMMENDED, None).about("Sends models to 3D printers; discontinued."),
    app("mixed-reality", "Mixed Reality Portal", "Microsoft.MixedReality.Portal", Microsoft, RECOMMENDED, None).about("For Windows Mixed Reality headsets, which are discontinued."),
    app("skype", "Skype", "Microsoft.SkypeApp", Microsoft, RECOMMENDED, None).about("Calls and chat; Skype has shut down."),
    app("messaging", "Microsoft Messaging", "Microsoft.Messaging", Microsoft, RECOMMENDED, None).about("Old text messaging app; no longer used."),
    app("one-connect", "Mobile Plans (OneConnect)", "Microsoft.OneConnect", Microsoft, RECOMMENDED, None).about("Buys mobile data for PCs with a SIM."),
    app("sway", "Office Sway", "Microsoft.Office.Sway", Microsoft, RECOMMENDED, None).about("Makes web presentations."),
    app("journal", "Microsoft Journal", "Microsoft.MicrosoftJournal", Microsoft, RECOMMENDED, None).about("Handwritten notes with a pen."),
    app("power-bi", "Power BI", "Microsoft.MicrosoftPowerBIForWindows", Microsoft, RECOMMENDED, None).about("Business reports and dashboards."),
    app("movies-tv", "Movies & TV", "Microsoft.ZuneVideo", Microsoft, RECOMMENDED, None).about("Plays and sells videos; Media Player replaces it."),
    app("maps", "Maps", "Microsoft.WindowsMaps", Microsoft, RECOMMENDED, None).about("Offline maps; Microsoft is retiring it."),
    app("family", "Microsoft Family", "MicrosoftCorporationII.MicrosoftFamily", Microsoft, RECOMMENDED, None).about("Parental controls for a Microsoft family group.").keep("Keep it if your family group uses parental controls."),
    app("teams-personal", "Microsoft Teams (personal)", "MicrosoftTeams", Microsoft, RECOMMENDED, None).about("Teams for family and friends (the old chat)."),
    app("teams", "Microsoft Teams", "MSTeams", Microsoft, MAXIMUM, None).about("Teams for work and school.").keep("Keep it if you use Teams for work or school."),
    app("pc-manager", "PC Manager", "Microsoft.MicrosoftPCManager", Microsoft, RECOMMENDED, None).about("Microsoft's cleanup and booster tool."),
    app("onenote-legacy", "OneNote for Windows 10", "Microsoft.Office.OneNote", Microsoft, RECOMMENDED, None).about("The old OneNote; the desktop OneNote replaces it."),
    app("people", "People", "Microsoft.People", Microsoft, RECOMMENDED, None).about("Contacts list; no longer used by Windows."),
    app("wallet", "Wallet", "Microsoft.Wallet", Microsoft, RECOMMENDED, None).about("Old payment app; no longer used."),
    app("speed-test", "Network Speed Test", "Microsoft.NetworkSpeedTest", Microsoft, RECOMMENDED, None).about("Tests the internet speed."),
    app("copilot", "Microsoft Copilot", "Microsoft.Copilot", Microsoft, RECOMMENDED, Some("9NHT9RB2F4HD")).about("Microsoft's AI chat.").keep("Keep it if you use Copilot."),
    // Microsoft, offered but kept unless ticked.
    app("mail-calendar", "Mail & Calendar", "microsoft.windowscommunicationsapps", Microsoft, OPT_IN, None).about("The old Mail and Calendar; the new Outlook replaces it.").keep("Keep it if you still read your email in it."),
    app("outlook", "Outlook (new)", "Microsoft.OutlookForWindows", Microsoft, OPT_IN, Some("9NRX63209R7B")).about("Email and calendar.").keep("Keep it if you read your email in it."),
    app("alarms", "Alarms & Clock", "Microsoft.WindowsAlarms", Microsoft, OPT_IN, Some("9WZDNCRFJ3PR")).about("Alarms, timers and world clock.").keep("Keep it for alarms and timers."),
    app("sound-recorder", "Sound Recorder", "Microsoft.WindowsSoundRecorder", Microsoft, MAXIMUM, Some("9WZDNCRFHWKN")).about("Records audio from the microphone.").keep("Keep it if you record audio."),
    app("sticky-notes", "Sticky Notes", "Microsoft.MicrosoftStickyNotes", Microsoft, OPT_IN, Some("9NBLGGH4QGHW")).about("Notes on the desktop.").keep("Keep it if you use notes on the desktop."),
    app("calculator", "Calculator", "Microsoft.WindowsCalculator", Microsoft, OPT_IN, Some("9WZDNCRFHVN5")).about("Everyday and scientific calculator.").keep("Most people use it."),
    app("camera", "Camera", "Microsoft.WindowsCamera", Microsoft, OPT_IN, Some("9WZDNCRFJBBG")).about("Takes photos and videos with the webcam.").keep("Keep it to use the webcam on its own."),
    app("photos", "Photos", "Microsoft.Windows.Photos", Microsoft, OPT_IN, Some("9WZDNCRFJBH4")).about("Views and edits pictures.").keep("Pictures open in it unless you use another viewer."),
    app("notepad", "Notepad", "Microsoft.WindowsNotepad", Microsoft, OPT_IN, Some("9MSMLRH6LZF3")).about("Plain text editor.").keep("Text files open in it unless you use another editor."),
    app("paint", "Paint", "Microsoft.Paint", Microsoft, OPT_IN, Some("9PCFS5B6T72H")).about("Simple drawing and picture editing.").keep("Keep it for quick drawings and edits."),
    app("snipping-tool", "Snipping Tool", "Microsoft.ScreenSketch", Microsoft, OPT_IN, Some("9MZ95KL8MR0L")).about("Screenshots (Win+Shift+S) and screen recording.").keep("Screenshots with Win+Shift+S need it."),
    app("media-player", "Media Player", "Microsoft.ZuneMusic", Microsoft, OPT_IN, Some("9WZDNCRFJ3PT")).about("Plays music and videos.").keep("Music and videos open in it unless you use another player."),
    app("phone-link", "Phone Link", "Microsoft.YourPhone", Microsoft, MAXIMUM, Some("9NMPJ99VJBWV")).about("Your phone's messages, calls and photos on the PC.").keep("Keep it to use your phone from the PC."),
    app("quick-assist", "Quick Assist", "MicrosoftCorporationII.QuickAssist", Microsoft, OPT_IN, Some("9P7BP5VNWKX5")).about("Lets someone help you over the internet.").keep("Keep it if someone helps you with your PC."),
    app("whiteboard", "Whiteboard", "Microsoft.Whiteboard", Microsoft, MAXIMUM, None).about("Drawing board for meetings."),
    app("widgets", "Widgets (Web Experience)", "MicrosoftWindows.Client.WebExperience", Microsoft, OPT_IN, Some("9MSSGKG348SP")).about("The Widgets board: weather, news and more.").keep("The Widgets board (Win+W) needs it."),
    // Bing.
    app("bing-news", "Bing News", "Microsoft.BingNews", Bing, RECOMMENDED, Some("9WZDNCRFHVFW")).about("News from Bing."),
    app("bing-weather", "Bing Weather", "Microsoft.BingWeather", Bing, RECOMMENDED, Some("9WZDNCRFJ3Q2")).about("Weather forecasts."),
    app("bing-finance", "Bing Finance", "Microsoft.BingFinance", Bing, RECOMMENDED, None).about("Stock prices; discontinued."),
    app("bing-sports", "Bing Sports", "Microsoft.BingSports", Bing, RECOMMENDED, None).about("Sports results; discontinued."),
    app("bing-travel", "Bing Travel", "Microsoft.BingTravel", Bing, RECOMMENDED, None).about("Travel guides; discontinued."),
    app("bing-food", "Bing Food & Drink", "Microsoft.BingFoodAndDrink", Bing, RECOMMENDED, None).about("Recipes; discontinued."),
    app("bing-health", "Bing Health & Fitness", "Microsoft.BingHealthAndFitness", Bing, RECOMMENDED, None).about("Fitness tracking; discontinued."),
    app("bing-translator", "Bing Translator", "Microsoft.BingTranslator", Bing, RECOMMENDED, None).about("Translations from Bing."),
    app("bing-search", "Bing Search", "Microsoft.BingSearch", Bing, RECOMMENDED, None).about("Bing web search in Start."),
    // Xbox: games may need them, so none is ticked.
    app("xbox-app", "Xbox", "Microsoft.GamingApp", Xbox, OPT_IN, Some("9MV0B5HZVK9Z")).about("Xbox games and PC Game Pass.").keep("Needed for PC Game Pass and many Xbox games."),
    app("xbox-game-bar", "Xbox Game Bar", "Microsoft.XboxGamingOverlay", Xbox, OPT_IN, Some("9NZKPSTSNW4P")).about("Win+G: game overlay, recording and chat.").keep("Some games and the Win+G recording need it."),
    app("xbox-companion", "Xbox Console Companion", "Microsoft.XboxApp", Xbox, RECOMMENDED, None).about("The old Xbox app; replaced by Xbox."),
    // Third-party apps Windows installs to promote them.
    promo("candy-crush", "Candy Crush Saga", ".CandyCrushSaga", None).about("Puzzle game installed as an ad."),
    promo("candy-crush-soda", "Candy Crush Soda Saga", ".CandyCrushSodaSaga", None).about("Puzzle game installed as an ad."),
    promo("candy-crush-friends", "Candy Crush Friends", ".CandyCrushFriends", None).about("Puzzle game installed as an ad."),
    promo("bubble-witch", "Bubble Witch 3 Saga", ".BubbleWitch3Saga", None).about("Puzzle game installed as an ad."),
    promo("farmville", "FarmVille 2", ".FarmVille2CountryEscape", None).about("Farming game installed as an ad."),
    promo("march-of-empires", "March of Empires", ".MarchofEmpires", None).about("Strategy game installed as an ad."),
    promo("asphalt", "Asphalt 8", ".Asphalt8Airborne", None).about("Racing game installed as an ad."),
    promo("hidden-city", "Hidden City", ".HiddenCityMysteryofShadows", None).about("Hidden-object game installed as an ad."),
    promo("royal-revolt", "Royal Revolt", ".RoyalRevolt2", None).about("Strategy game installed as an ad."),
    promo("disney-kingdoms", "Disney Magic Kingdoms", ".DisneyMagicKingdoms", None).about("Park-building game installed as an ad."),
    promo("cooking-fever", "Cooking Fever", ".COOKINGFEVER", None).about("Cooking game installed as an ad."),
    promo("netflix", "Netflix", ".Netflix", None).about("Netflix, installed as an ad; the website works too."),
    promo("disney-plus", "Disney+", ".37853FC22B2CE", None).about("Disney+, installed as an ad; the website works too."),
    promo("prime-video", "Prime Video", ".PrimeVideo", None).about("Prime Video, installed as an ad; the website works too."),
    promo("amazon", "Amazon", ".Amazon", None).about("Amazon shopping, installed as an ad."),
    promo("tiktok", "TikTok", ".TikTok", None).about("TikTok, installed as an ad."),
    App { id: "instagram", title: "Instagram", name: Name::Contains(".Instagram"), group: AppGroup::ThirdParty, level: LIGHT, store_id: None, about: "Instagram, installed as an ad.", keep: None },
    promo("facebook", "Facebook", ".Facebook", None).about("Facebook, installed as an ad."),
    promo("twitter", "Twitter / X", ".Twitter", None).about("X (Twitter), installed as an ad."),
    promo("linkedin", "LinkedIn", ".LinkedInforWindows", None).about("LinkedIn, installed as an ad."),
    promo("pandora", "Pandora", ".29680B314EFC2", None).about("Music streaming, installed as an ad."),
    promo("tunein", "TuneIn Radio", ".TuneInRadio", None).about("Internet radio, installed as an ad."),
    promo("plex", "Plex", ".Plex", None).about("Media server client, installed as an ad."),
    promo("iheartradio", "iHeartRadio", ".iHeartRadio", None).about("Internet radio, installed as an ad."),
    promo("shazam", "Shazam", ".Shazam", None).about("Recognises songs; installed as an ad."),
    promo("hulu", "Hulu", ".HULUPLUS", None).about("Video streaming, installed as an ad."),
    promo("sling", "Sling TV", ".SlingTV", None).about("TV streaming, installed as an ad."),
    promo("duolingo", "Duolingo", ".Duolingo-LearnLanguagesforFree", None).about("Language lessons, installed as an ad."),
    promo("flipboard", "Flipboard", ".Flipboard", None).about("News magazine, installed as an ad."),
    promo("photoshop-express", "Adobe Photoshop Express", ".AdobePhotoshopExpress", None).about("Photo editor, installed as an ad."),
    promo("picsart", "PicsArt", ".PicsArt-PhotoStudio", None).about("Photo editor, installed as an ad."),
    promo("sketchbook", "SketchBook", ".AutodeskSketchBook", None).about("Drawing app, installed as an ad."),
    promo("drawboard", "Drawboard PDF", ".DrawboardPDF", None).about("PDF editor, installed as an ad."),
    promo("winzip", "WinZip", ".WinZipUniversal", None).about("Zip tool, installed as an ad; Windows opens zip files itself."),
    promo("viber", "Viber", ".Viber", None).about("Chat app, installed as an ad."),
    promo("xing", "XING", ".XING", None).about("Job network, installed as an ad."),
    promo("wunderlist", "Wunderlist", ".Wunderlist", None).about("To-do lists; discontinued."),
    // Added later: retired or promoted apps Windows still ships with.
    app("paint-3d", "Paint 3D", "Microsoft.MSPaint", Microsoft, RECOMMENDED, None).about("3D drawing; discontinued (Paint stays)."),
    app("cross-device", "Cross Device Experience Host", "MicrosoftWindows.CrossDevice", Microsoft, OPT_IN, None)
        .about("Connects a phone to Start and File Explorer.")
        .keep("Phone Link and the phone panel in Start need it."),
    app("game-assist", "Game Assist", "Microsoft.Edge.GameAssist", Xbox, MAXIMUM, None).about("Browser overlay for games (Game Bar)."),
    app("xbox-game-overlay", "Xbox Game Overlay (old)", "Microsoft.XboxGameOverlay", Xbox, OPT_IN, None)
        .about("Part of the older Game Bar.")
        .keep("Some games still use it."),
    App { id: "whatsapp", title: "WhatsApp", name: Name::Suffix(".WhatsAppDesktop"), group: AppGroup::ThirdParty, level: MAXIMUM, store_id: Some("9NKSQGP7F2NH"), about: "Chat app.", keep: Some("Keep it if you chat on WhatsApp from the PC.") },
    App { id: "messenger", title: "Messenger", name: Name::Exact("FACEBOOK.317180B0BB486"), group: AppGroup::ThirdParty, level: LIGHT, store_id: None, about: "Facebook Messenger, installed as an ad.", keep: None },
    App { id: "dolby-access", title: "Dolby Access", name: Name::Exact("DolbyLaboratories.DolbyAccess"), group: AppGroup::ThirdParty, level: OPT_IN, store_id: Some("9N0866FS04W8"), about: "Dolby Atmos sound settings.", keep: Some("Keep it if your PC uses Dolby Atmos.") },
    App { id: "spotify", title: "Spotify", name: Name::Exact("SpotifyAB.SpotifyMusic"), group: AppGroup::ThirdParty, level: MAXIMUM, store_id: Some("9NCBCSZSJRSB"), about: "Music streaming.", keep: Some("Keep it if you listen to Spotify.") },
];
/// Packages that are never removed, whatever the tables above say: Windows
/// or MYLE itself needs them, or they cannot easily be installed again.
pub const NEVER: &[&str] = &[
    "Microsoft.WindowsStore",
    "Microsoft.StorePurchaseApp",
    "Microsoft.DesktopAppInstaller",
    "Microsoft.WindowsTerminal",
    "Microsoft.Xbox.TCUI",
    "Microsoft.XboxIdentityProvider",
    "Microsoft.XboxSpeechToTextOverlay",
    "Microsoft.GetHelp",
    "Microsoft.SecHealthUI",
    "Microsoft.WebView2",
    "Microsoft.Win32WebViewHost",
    "Microsoft.MicrosoftEdge.Stable",
    "Microsoft.MicrosoftEdge",
    "Microsoft.MicrosoftEdgeDevToolsClient",
    "Microsoft.AAD.BrokerPlugin",
    "Microsoft.AccountsControl",
    "Microsoft.LockApp",
    "Microsoft.Windows.ShellExperienceHost",
    "Microsoft.Windows.StartMenuExperienceHost",
    "MicrosoftWindows.Client.CBS",
    "MicrosoftWindows.Client.Core",
    "Microsoft.WindowsAppRuntime.1.5",
    "Microsoft.VCLibs.140.00",
    "Microsoft.VCLibs.140.00.UWPDesktop",
    "Microsoft.UI.Xaml.2.8",
    "Microsoft.NET.Native.Framework.2.2",
    "Microsoft.NET.Native.Runtime.2.2",
    "Microsoft.HEIFImageExtension",
    "Microsoft.HEVCVideoExtension",
    "Microsoft.WebpImageExtension",
    "Microsoft.VP9VideoExtensions",
    "Microsoft.RawImageExtension",
    "Microsoft.ApplicationCompatibilityEnhancements",
];
/// Whether `package` may be removed at all: it is in the table, and is not
/// something Windows or MYLE needs. The administrator helper checks this
/// for every name it is sent.
pub fn removable(package: &str) -> Option<&'static App> {
    let safe_chars = !package.is_empty()
        && package.len() <= 128
        && package.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_');
    if !safe_chars || is_never(package) {
        return None;
    }
    APPS.iter().find(|app| app.name.matches(package))
}
fn is_never(package: &str) -> bool {
    let lower = package.to_ascii_lowercase();
    NEVER.iter().any(|never| lower == never.to_ascii_lowercase())
        || lower.contains("webview")
        || lower.starts_with("microsoft.vclibs")
        || lower.starts_with("microsoft.ui.xaml")
        || lower.starts_with("microsoft.net.")
        || lower.starts_with("microsoft.windowsappruntime")
        || lower.ends_with("extension")
        || lower.ends_with("extensions")
}
pub fn find_app(id: &str) -> Option<&'static App> {
    APPS.iter().find(|app| app.id == id)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    #[test]
    fn ids_are_unique_and_tidy() {
        let mut seen = HashSet::new();
        for tweak in TWEAKS {
            assert!(seen.insert(tweak.id), "duplicate tweak {}", tweak.id);
            assert!(tweak.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
            assert!(!tweak.ops.is_empty(), "{} does nothing", tweak.id);
        }
        let mut seen = HashSet::new();
        for app in APPS {
            assert!(seen.insert(app.id), "duplicate app {}", app.id);
        }
    }
    #[test]
    fn registry_paths_are_relative_and_values_sane() {
        for tweak in TWEAKS {
            for op in tweak.ops {
                if let Op::Reg { path, name, data, .. } = op {
                    assert!(!path.starts_with('\\') && !path.contains("HKEY") && !path.contains('/'), "{path}");
                    assert!(!path.ends_with('\\'), "{path}");
                    // The helper's strings must be ones undo accepts back.
                    if let (false, Data::Sz(value)) = (op.as_user(), data) {
                        assert!(allowed_helper_strings(path, name).contains(value), "{path}\\{name}");
                    }
                }
                if let Op::Service { to, from, windows, .. } = op {
                    assert!(!from.contains(to), "{}: nothing to change", tweak.id);
                    assert!(windows == to || from.contains(windows), "{}: Windows' own start type", tweak.id);
                }
            }
        }
    }
    #[test]
    fn windows_features_name_only_their_own_capability() {
        let mut features = 0;
        for tweak in TWEAKS {
            for op in tweak.ops {
                if let Op::Capability { name, package } = op {
                    features += 1;
                    assert_eq!(tweak.category, Category::Features, "{}", tweak.id);
                    assert!(name.ends_with("~~~~") && package.ends_with('~'), "{}", tweak.id);
                    assert!(name.bytes().chain(package.bytes()).all(|b| b.is_ascii_alphanumeric() || b"~.-".contains(&b)), "{}", tweak.id);
                }
            }
        }
        assert!(features >= 6);
    }
    #[test]
    fn every_app_says_what_it_is() {
        for app in APPS {
            assert!(app.about.ends_with('.') && app.about.len() > 4, "{}: {:?}", app.id, app.about);
            if let Some(keep) = app.keep {
                assert!(keep.ends_with('.'), "{}", app.id);
            }
        }
        // What most people use is never removed without a word on why to keep it.
        for id in ["calculator", "photos", "notepad", "snipping-tool", "xbox-app", "xbox-game-bar", "phone-link"] {
            assert!(find_app(id).unwrap().keep.is_some(), "{id}");
        }
    }
    #[test]
    fn nothing_windows_or_myle_needs_can_be_removed() {
        for never in NEVER {
            assert!(removable(never).is_none(), "{never}");
        }
        for app in APPS {
            if let Name::Exact(name) = app.name {
                assert!(removable(name).is_some(), "{name} should be removable");
            }
        }
        assert!(removable("Microsoft.WebView2Runtime").is_none());
        assert!(removable("Microsoft.VCLibs.140.00.UWPDesktop").is_none());
        assert!(removable("Microsoft.AV1VideoExtension").is_none());
        assert!(removable("Evil;rm -rf").is_none());
        assert!(removable("").is_none());
    }
    #[test]
    fn third_party_names_match_only_their_own_package() {
        assert!(removable("king.com.CandyCrushSaga").is_some());
        assert!(removable("king.com.CandyCrushSodaSaga").is_some_and(|app| app.id == "candy-crush-soda"));
        assert!(removable("4DF9E0F8.Netflix").is_some());
        assert!(removable("Netflix").is_none(), "a bare suffix is not a package");
        assert!(removable("Microsoft.WindowsCalculator").is_some_and(|app| app.level.is_none()));
    }
    #[test]
    fn the_maximum_profile_keeps_what_the_one_click_debloat_did() {
        let maximum: Vec<&str> = TWEAKS.iter().filter(|t| t.level.is_some()).map(|t| t.id).collect();
        for asked in [
            "telemetry", "clock-24h", "background-apps", "services", "copilot",
            "location", "taskbar-search", "end-task", "taskview-widgets", "bing", "classic-context-menu",
        ] {
            assert!(maximum.contains(&asked), "{asked}");
        }
        // Edge goes only when picked on its own, and asks again then.
        assert_eq!(find("edge").unwrap().level, None);
        assert!(find("edge").unwrap().confirm.is_some(), "Edge asks again");
    }
    #[test]
    fn profiles_build_on_each_other_and_stay_careful() {
        assert!(Level::Light < Level::Recommended && Level::Recommended < Level::Maximum);
        for tweak in TWEAKS {
            // A tweak that asks again, or is marked caution, is never in a profile.
            if tweak.confirm.is_some() || tweak.risk == Risk::Caution {
                assert_eq!(tweak.level, None, "{}", tweak.id);
            }
            if let Some(note) = tweak.note {
                assert!(note.ends_with('.') && !note.is_empty(), "{}", tweak.id);
            }
        }
        for app in APPS {
            // Apps the Xbox games or the Store need are never removed by a profile.
            if matches!(app.id, "xbox-app" | "xbox-game-bar" | "calculator" | "photos" | "snipping-tool" | "notepad") {
                assert_eq!(app.level, None, "{}", app.id);
            }
        }
        // Maximum removes every promoted app, and the ones the user chose for it.
        for id in ["phone-link", "sound-recorder", "teams", "spotify", "whatsapp", "netflix", "tiktok"] {
            assert!(find_app(id).unwrap().level.is_some(), "{id}");
        }
        assert_eq!(find_app("mail-calendar").unwrap().level, None);
        let light = TWEAKS.iter().filter(|t| t.level == LIGHT).count();
        assert!(light >= 3, "Light has something to do");
    }
}
