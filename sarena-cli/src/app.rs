use anyhow::Result;
use sarena_api_client::{ApiClient, TransportKind};

use crate::{
    cli::{Commands, completion, service, version},
    config::Config,
};

pub struct App {
    pub _config: Config,
    pub client: ApiClient<TransportKind>,
}

impl App {
    pub async fn run(&self, command: &Commands) -> Result<()> {
        match command {
            Commands::Completion(args) => completion::run(args),
            Commands::Service(service) => service::run(service),
            Commands::Version => version::run(self).await,
        }
    }
}
