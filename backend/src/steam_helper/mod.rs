//! Steam Game Helper: what is free on Steam now, twelve weekly picks and every deal, read from
//! Steam's public store with no sign-in. Owner-only (`account::owner`).
pub(crate) mod cache;
pub(crate) mod commands;
pub(crate) mod model;
pub(crate) mod parse;
pub(crate) mod picks;
pub(crate) mod sections;
pub(crate) mod store;
