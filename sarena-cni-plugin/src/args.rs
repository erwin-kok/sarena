use std::collections::HashMap;

use rscni_plugin::error::Error;
use serde::{Deserialize, de::DeserializeOwned};

use crate::Res;

#[derive(Debug, Deserialize, Default)]
pub struct ArgsSpec {
    #[serde(rename = "K8S_POD_NAME")]
    pub k8s_pod_name: String,
    #[serde(rename = "K8S_POD_NAMESPACE")]
    pub k8s_pod_namespace: String,
    #[serde(rename = "K8S_POD_UID")]
    pub _k8s_pod_uid: String,
}

pub fn load_args<T>(args: Option<&String>) -> Res<T>
where
    T: DeserializeOwned + Default,
{
    // If no args → return default
    let args = match args {
        None => return Ok(T::default()),
        Some(s) if s.trim().is_empty() => return Ok(T::default()),
        Some(s) => s,
    };

    let mut map: HashMap<String, serde_json::Value> = HashMap::new();

    for kv in args.split(';') {
        let mut parts = kv.splitn(2, '=');

        let key = parts
            .next()
            .ok_or_else(|| Error::InvalidNetworkConfig(format!("ARGS: missing key in '{kv}'")))?;

        let value = parts
            .next()
            .ok_or_else(|| Error::InvalidNetworkConfig(format!("ARGS: missing value in '{kv}'")))?;

        // Try to parse as JSON value (number, bool, etc.)
        let json_value = match value.parse::<serde_json::Value>() {
            Ok(v) => v,
            Err(_) => serde_json::Value::String(value.to_string()),
        };

        map.insert(key.to_string(), json_value);
    }

    serde_json::from_value(serde_json::to_value(map).unwrap())
        .map_err(|e| Error::InvalidNetworkConfig(format!("ARGS: deserialization error: {e}")))
}
