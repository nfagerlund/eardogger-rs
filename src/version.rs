// written by build.rs.
// First line is commit sha, second line is date of build.
pub const VERSION_DATA: &str = include_str!("../VERSION.txt");

pub fn commit_sha() -> &'static str {
    VERSION_DATA.lines().next().unwrap_or("")
}

pub fn build_date() -> &'static str {
    VERSION_DATA.lines().nth(1).unwrap_or("")
}
