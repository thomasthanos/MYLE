//! The website a login is for, as the browser and the icons see it.

/// The host of a saved login's website (`www.` and a final dot dropped), or
/// `None` when it is not a web address.
pub fn saved_host(value: &str) -> Option<String> {
    let address = if value.contains("://") { value.to_string() } else { format!("https://{value}") };
    let parsed = reqwest::Url::parse(&address).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    Some(parsed.host_str()?.trim_end_matches('.').trim_start_matches("www.").to_lowercase())
}
