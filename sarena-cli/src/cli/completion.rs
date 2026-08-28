use clap::{Args, CommandFactory as _};
use clap_complete::{generate, shells};

use crate::cli::Cli;

#[derive(Args, Debug)]
pub struct CompletionArgs {
    #[arg(value_parser = ["bash", "zsh", "fish"])]
    pub shell: String,
}

pub fn run(args: &CompletionArgs) {
    let mut cmd = Cli::command();
    match args.shell.as_str() {
        "bash" => generate(shells::Bash, &mut cmd, "sarena-cli", &mut std::io::stdout()),
        "zsh" => generate(shells::Zsh, &mut cmd, "sarena-cli", &mut std::io::stdout()),
        "fish" => generate(shells::Fish, &mut cmd, "sarena-cli", &mut std::io::stdout()),
        _ => unreachable!("value_parser restricts shell to bash/zsh/fish"),
    }
}
