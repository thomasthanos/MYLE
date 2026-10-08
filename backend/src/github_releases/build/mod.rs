//! Building a project: which command (`detect`), running it in a Job Object
//! so Cancel stops every process it started (`runner`), reading errors,
//! warnings and stages from what it prints (`parse`), and finding the files
//! it made (`artifacts`).

pub mod artifacts;
pub mod detect;
pub mod parse;
pub mod runner;
