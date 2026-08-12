#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    #[error("packet too short: missing {0}")]
    PacketSizeError(&'static str),
}

pub(crate) type Res<T> = Result<T, CommonError>;
