#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    #[error("packet too short")]
    PacketSizeError,
}

pub(crate) type Res<T> = Result<T, CommonError>;
