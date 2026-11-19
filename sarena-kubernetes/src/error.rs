#[derive(Debug, thiserror::Error)]
pub enum KubernetesError {
    #[error("kube error: {0}")]
    Kube(#[from] kube::Error),

    #[error("failed to load in-cluster configuration: {0}")]
    InCluster(#[from] kube::config::InClusterError),

    #[error("failed to load kubeconfig: {0}")]
    Kubeconfig(#[from] kube::config::KubeconfigError),

    #[error("failed to infer Kubernetes client configuration: {0}")]
    InferConfig(#[from] kube::config::InferConfigError),

    #[error("invalid API server URL {url:?}: {source}")]
    InvalidApiServerUrl {
        url: String,
        #[source]
        source: http::uri::InvalidUri,
    },

    #[error("controller task panicked: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("controller exited unexpectedly")]
    ControllerExited,

    #[error("all controllers exited unexpectedly")]
    AllControllersExited,
}

pub type Res<T> = Result<T, KubernetesError>;
