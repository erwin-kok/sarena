use network_types::tcp::TcpHdr;

#[derive(Clone, Copy)]
pub struct TcpFlags {
    pub syn: bool,
    pub ack: bool,
    pub fin: bool,
    pub rst: bool,
}

impl TcpFlags {
    #[inline(always)]
    pub fn from(tcp: &TcpHdr) -> Self {
        Self {
            syn: tcp.syn() != 0,
            ack: tcp.ack() != 0,
            fin: tcp.fin() != 0,
            rst: tcp.rst() != 0,
        }
    }
}
