//! Taskbar selection and an outstanding pin request are separate preferences.
pub const TASKBAR_CHOICE: &str = "TaskbarShortcut";
pub const TASKBAR_PENDING: &str = "TaskbarPinPending";
#[derive(Debug, PartialEq, Eq)]
pub struct TaskbarPreference {
    pub enabled: bool,
    pub pending: bool,
}
pub fn taskbar_preference(choice: Option<bool>, pending: Option<bool>) -> TaskbarPreference {
    // Older setups saved only the outstanding offer. Preserve an old opt-out when migrating, but never interpret a completed request as a new opt-out.
    let enabled = choice.or(pending).unwrap_or(true);
    TaskbarPreference { enabled, pending: enabled && pending.unwrap_or(false) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_opt_out_blocks_even_an_old_pending_request() {
        assert_eq!(taskbar_preference(Some(false), Some(true)), TaskbarPreference { enabled: false, pending: false });
        assert_eq!(taskbar_preference(Some(false), None), TaskbarPreference { enabled: false, pending: false });
    }
    #[test]
    fn updates_keep_the_selection_after_a_pin_request_was_handled() {
        assert_eq!(taskbar_preference(Some(true), Some(false)), TaskbarPreference { enabled: true, pending: false });
        assert_eq!(taskbar_preference(Some(true), Some(true)), TaskbarPreference { enabled: true, pending: true });
    }
    #[test]
    fn a_new_default_is_selected_without_requesting_a_pin_until_setup_asks() {
        assert_eq!(taskbar_preference(None, None), TaskbarPreference { enabled: true, pending: false });
    }
    #[test]
    fn legacy_unchecked_taskbar_choices_are_not_turned_back_on() {
        assert_eq!(taskbar_preference(None, Some(false)), TaskbarPreference { enabled: false, pending: false });
        assert_eq!(taskbar_preference(None, Some(true)), TaskbarPreference { enabled: true, pending: true });
    }
}
