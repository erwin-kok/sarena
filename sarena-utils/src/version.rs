pub fn version(version: &'static str, git_hash: &'static str, build_date: &'static str) -> String {
    format!("{version} ({git_hash}, built {build_date})")
}
