use std::str::FromStr;

use anyhow::Result;
use clap::Args;
use serde::Serialize;
use serde_json::Value;

#[derive(Args, Debug)]
pub struct OutputArgs {
    /// Render the result as machine-readable output instead of a table:
    /// `json`, `yaml`, or `jsonpath=<expression>`.
    #[arg(short = 'o', long)]
    pub output: Option<OutputFormat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Yaml,
    JsonPath(String),
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "json" => Ok(Self::Json),
            "yaml" => Ok(Self::Yaml),
            value => {
                if let Some(expression) = value.strip_prefix("jsonpath=") {
                    Ok(Self::JsonPath(expression.to_string()))
                } else {
                    Err(format!(
                        "invalid output format: {value}; expected json, yaml, or jsonpath=<expression>"
                    ))
                }
            }
        }
    }
}

pub fn print_output<T>(data: &T, output: &OutputFormat) -> Result<()>
where
    T: Serialize,
{
    match output {
        OutputFormat::Json => dump_json(data),
        OutputFormat::Yaml => dump_yaml(data),
        OutputFormat::JsonPath(expression) => dump_json_path(data, expression),
    }
}

fn dump_json<T>(data: &T) -> Result<()>
where
    T: Serialize,
{
    let result = serde_json::to_string_pretty(data)?;
    println!("{result}");

    Ok(())
}

fn dump_yaml<T>(data: &T) -> Result<()>
where
    T: Serialize,
{
    let result = serde_yaml::to_string(data)?;
    print!("{result}");

    Ok(())
}

fn dump_json_path<T>(data: &T, expression: &str) -> Result<()>
where
    T: Serialize,
{
    let value: Value = serde_json::to_value(data)?;

    let jsonpath = jsonpath_lib::select(&value, expression)
        .map_err(|err| anyhow::anyhow!("couldn't parse jsonpath expression: {err}"))?;

    for result in jsonpath {
        println!("{}", serde_json::to_string(result)?);
    }

    Ok(())
}
