use rscni_plugin::error::Error;

mod args;
mod client;
mod cmd;
mod plugin;

pub use plugin::SarenaPlugin;

pub type Res<T> = Result<T, Error>;
