use aya_ebpf::{
    bindings::tcx_action_base::{TCX_DROP, TCX_NEXT, TCX_PASS},
    macros::map,
    maps::PerCpuArray,
};
use network_types::ip::IpError;
use sarena_ebpf_common::CommonError;

#[map(name = "prog_errors")]
static ERRORS: PerCpuArray<u64> = PerCpuArray::pinned(16, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Hand the packet back to the stack (`TCX_PASS`).
    Pass,
    /// Drop the packet (`TCX_DROP`).
    Drop,
    /// Fall through to the next tc/tcx filter (`TCX_NEXT`).
    Next,
    /// A raw action code from a helper that already returns one
    /// (`bpf_redirect` / `bpf_redirect_peer`).
    Redirect(i32),
}

impl From<Verdict> for i32 {
    fn from(v: Verdict) -> Self {
        match v {
            Verdict::Pass => TCX_PASS,
            Verdict::Drop => TCX_DROP,
            Verdict::Next => TCX_NEXT,
            Verdict::Redirect(code) => code,
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

    #[error("TTL exceeded")]
    TtlExceeded,

    #[error("L3 checksum update failed")]
    CsumL3,
}

impl EbpfError {
    pub const fn verdict(&self) -> Verdict {
        match self {
            EbpfError::Common(_)
            | EbpfError::InternalError(_)
            | EbpfError::IpError(_)
            | EbpfError::TtlExceeded
            | EbpfError::CsumL3 => Verdict::Drop,

            EbpfError::UnsupportedProtocol(_) => Verdict::Pass,
        }
    }

    pub const fn code(&self) -> u32 {
        match self {
            EbpfError::Common(_) => 1,
            EbpfError::InternalError(_) => 2,
            EbpfError::IpError(_) => 3,
            EbpfError::UnsupportedProtocol(_) => 4,
            EbpfError::TtlExceeded => 5,
            EbpfError::CsumL3 => 6,
        }
    }
}

pub type Res<T> = Result<T, EbpfError>;

#[inline(always)]
pub fn dispatch(result: Res<Verdict>) -> i32 {
    match result {
        Ok(v) => v.into(),
        Err(e) => {
            if let Some(slot) = ERRORS.get_ptr_mut(e.code()) {
                unsafe { *slot += 1 };
            }
            e.verdict().into()
        }
    }
}
