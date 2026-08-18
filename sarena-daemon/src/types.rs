use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdArgs {
    pub container_id: String,
    pub netns: PathBuf,
    pub if_name: String,
    pub args: Option<String>,
    pub path: String,
    pub stdin_data: Vec<u8>,
    pub netns_override: Option<String>,
}
