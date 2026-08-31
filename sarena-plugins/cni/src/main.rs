use rscni_plugin::{async_cni::Plugin, version::SpecVersion};
use sarena_cni_plugin::SarenaPlugin;
use sarena_utils::logging;

const CNI_VERSION: SpecVersion = SpecVersion::new(1, 3, 0);
const SUPPORTED_VERSIONS: [SpecVersion; 8] = [
    SpecVersion::new(0, 1, 0),
    SpecVersion::new(0, 2, 0),
    SpecVersion::new(0, 3, 0),
    SpecVersion::new(0, 3, 1),
    SpecVersion::new(0, 4, 0),
    SpecVersion::new(1, 0, 0),
    SpecVersion::new(1, 1, 0),
    SpecVersion::new(1, 3, 0),
];

const ABOUT_MSG: &str = concat!("Sarena CNI plugin ", env!("CARGO_PKG_VERSION"));

#[tokio::main]
async fn main() {
    let plugin = Plugin::new(CNI_VERSION, SUPPORTED_VERSIONS.to_vec()).msg(ABOUT_MSG);
    let sarena_plugin = SarenaPlugin;
    let result = plugin.run(&sarena_plugin).await;
    logging::shutdown_logging();
    if let Err(e) = result {
        let code = u32::from(&e);
        eprintln!("{e}: {}", e.details());
        std::process::exit(i32::try_from(code).unwrap_or(1));
    }
}
