use std::collections::BTreeMap;

use k8s_openapi::{
    ByteString,
    api::{
        admissionregistration::v1::{
            RuleWithOperations, ServiceReference, ValidatingWebhook,
            ValidatingWebhookConfiguration, WebhookClientConfig,
        },
        core::v1::Secret,
    },
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::Resource;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use sarena_kubernetes::{
    crd::address_pool::AddressPool,
    webhook::{ADDRESS_POOL_VALIDATE_PATH, NAMESPACE, SECRET_NAME, SERVICE_NAME},
};

const WEBHOOK_CONFIGURATION_NAME: &str = "addresspool.sarena.erwinkok.org";

fn main() {
    let CertifiedKey { cert, signing_key } = generate_simple_self_signed(vec![
        format!("{SERVICE_NAME}.{NAMESPACE}.svc"),
        format!("{SERVICE_NAME}.{NAMESPACE}.svc.cluster.local"),
    ])
    .expect("failed to generate self-signed webhook certificate");

    let cert_pem = cert.pem();
    let key_pem = signing_key.serialize_pem();

    println!("---");
    print!(
        "{}",
        serde_yaml::to_string(&secret(&cert_pem, &key_pem)).unwrap()
    );
    println!("---");
    print!(
        "{}",
        serde_yaml::to_string(&address_pool(&cert_pem)).unwrap()
    );
}

fn secret(cert_pem: &str, key_pem: &str) -> Secret {
    Secret {
        metadata: ObjectMeta {
            name: Some(SECRET_NAME.to_string()),
            namespace: Some(NAMESPACE.to_string()),
            ..Default::default()
        },
        type_: Some("kubernetes.io/tls".to_string()),
        data: Some(BTreeMap::from([
            (
                "tls.crt".to_string(),
                ByteString(cert_pem.as_bytes().to_vec()),
            ),
            (
                "tls.key".to_string(),
                ByteString(key_pem.as_bytes().to_vec()),
            ),
        ])),
        ..Default::default()
    }
}

fn address_pool(cert_pem: &str) -> ValidatingWebhookConfiguration {
    ValidatingWebhookConfiguration {
        metadata: ObjectMeta {
            name: Some(WEBHOOK_CONFIGURATION_NAME.to_string()),
            ..Default::default()
        },
        webhooks: Some(vec![ValidatingWebhook {
            name: WEBHOOK_CONFIGURATION_NAME.to_string(),
            admission_review_versions: vec!["v1".to_string()],
            side_effects: "None".to_string(),
            match_policy: Some("Equivalent".to_string()),
            failure_policy: Some("Fail".to_string()),
            timeout_seconds: Some(5),
            client_config: WebhookClientConfig {
                ca_bundle: Some(ByteString(cert_pem.as_bytes().to_vec())),
                service: Some(ServiceReference {
                    name: SERVICE_NAME.to_string(),
                    namespace: NAMESPACE.to_string(),
                    path: Some(ADDRESS_POOL_VALIDATE_PATH.to_string()),
                    port: Some(443),
                }),
                url: None,
            },
            rules: Some(vec![RuleWithOperations {
                api_groups: Some(vec![AddressPool::group(&()).to_string()]),
                api_versions: Some(vec![AddressPool::version(&()).to_string()]),
                operations: Some(vec!["CREATE".to_string(), "UPDATE".to_string()]),
                resources: Some(vec![AddressPool::plural(&()).to_string()]),
                scope: Some("Cluster".to_string()),
            }]),
            namespace_selector: None,
            object_selector: None,
            match_conditions: None,
        }]),
    }
}
