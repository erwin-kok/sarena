use aya_ebpf::bindings::tcx_action_base::{TCX_DROP, TCX_NEXT, TCX_PASS};
use network_types::ip::IpError;
use sarena_ebpf_common::CommonError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EbpfReturn {
    Pass,
    Drop,
    Next,
    /// Escape hatch for helpers that hand back a raw action code directly,
    /// e.g. `bpf_redirect_peer`'s return value.
    Custom(i32),
}

impl From<EbpfReturn> for i32 {
    fn from(ret: EbpfReturn) -> Self {
        match ret {
            EbpfReturn::Pass => TCX_PASS,
            EbpfReturn::Drop => TCX_DROP,
            EbpfReturn::Next => TCX_NEXT,
            EbpfReturn::Custom(v) => v,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EbpfError {
    #[error(transparent)]
    Common(#[from] CommonError),

    #[error("Internal Error: {0}")]
    InternalError(&'static str),

    #[error("IpError: {0}")]
    IpError(#[from] IpError),

    #[error("Protocol not supported: {0}")]
    UnsupportedProtocol(u8),
}

pub type Res<T> = Result<T, EbpfError>;
