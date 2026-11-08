//! Steam Automation: card farming, playtime idling, achievement unlocking and selling duplicate
//! cards, through the Steam client signed in on this PC. Owner-only (`account::owner`).
pub(crate) mod client;
pub(crate) mod commands;
pub(crate) mod helper;
pub(crate) mod runner;
pub(crate) mod session;
pub(crate) mod steam_state;
