use rscni_plugin::async_cni::Plugin;
use sarena_cni_plugin::SarenaPlugin;
use sarena_utils::logging;

const CNI_VERSION: &str = "1.3.0";
const SUPPORTED_VERSIONS: [&str; 8] = [
    "0.1.0", "0.2.0", "0.3.0", "0.3.1", "0.4.0", "1.0.0", "1.1.0", "1.3.0",
];
const ABOUT_MSG: &str = "Sarena CNI plugin 0.1.0";

#[tokio::main]
async fn main() {
    let supported_versions: Vec<String> =
        SUPPORTED_VERSIONS.iter().map(ToString::to_string).collect();
    let plugin = Plugin::new(CNI_VERSION, supported_versions).msg(ABOUT_MSG);
    let sarena_plugin = SarenaPlugin;
    let result = plugin.run(&sarena_plugin).await;
    logging::shutdown_logging();
    if let Err(e) = result {
        let code = u32::from(&e);
        eprintln!("{e}: {}", e.details());
        std::process::exit(i32::try_from(code).unwrap_or(1));
    }
}
