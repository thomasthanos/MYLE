//! Steam Automation: card farming, playtime idling, achievement unlocking and selling duplicate
//! cards, through the Steam client signed in on this PC. Owner-only (`account::owner`).
pub(crate) mod achievements;
pub(crate) mod badges;
pub(crate) mod claim;
pub(crate) mod client;
pub(crate) mod commands;
pub(crate) mod community;
pub(crate) mod decide;
pub(crate) mod engine;
pub(crate) mod helper;
pub(crate) mod inventory;
pub(crate) mod library;
pub(crate) mod runner;
pub(crate) mod session;
pub(crate) mod settings;
pub(crate) mod steam_state;
