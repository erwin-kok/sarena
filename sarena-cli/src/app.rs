use anyhow::Result;
use sarena_api_client::{ApiClient, TransportKind};

use crate::{
    cli::{Commands, bpf, completion, service, version},
    config::Config,
};

pub struct App {
    pub config: Config,
    pub client: ApiClient<TransportKind>,
}

impl App {
    pub async fn run(&self, command: &Commands) -> Result<()> {
        match command {
            Commands::Bpf(command) => bpf::run(self, command)?,
            Commands::Completion(args) => completion::run(args),
            Commands::Service(command) => service::run(command),
            Commands::Version(args) => version::run(self, args.output.as_ref()).await?,
        }
        Ok(())
    }
}
