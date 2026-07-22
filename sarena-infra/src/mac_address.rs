use std::fmt;

use crate::InfraError;

/// Six-octet Ethernet hardware address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Parse a colon-separated MAC string such as `"aa:bb:cc:dd:ee:ff"`.
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<u8> = s
            .split(':')
            .filter_map(|x| u8::from_str_radix(x, 16).ok())
            .collect();
        if parts.len() == 6 {
            let mut arr = [0u8; 6];
            arr.copy_from_slice(&parts);
            Some(Self(arr))
        } else {
            None
        }
    }
}

impl TryFrom<&[u8]> for MacAddress {
    type Error = InfraError;
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        if b.len() != 6 {
            return Err(InfraError::InvalidMac(b.len()));
        }
        let mut arr = [0u8; 6];
        arr.copy_from_slice(b);
        Ok(Self(arr))
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        )
    }
}

impl fmt::Debug for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
