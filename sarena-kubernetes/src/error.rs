#[derive(Debug, thiserror::Error)]
pub enum KubernetesError {
    #[error("kube error: {0}")]
    Kube(#[from] kube::Error),

    #[error("controller task panicked: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("controller exited unexpectedly")]
    ControllerExited,

    #[error("all controllers exited unexpectedly")]
    AllControllersExited,
}

pub type Res<T> = Result<T, KubernetesError>;
