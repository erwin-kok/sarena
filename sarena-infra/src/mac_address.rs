use std::fmt;

use rand::RngExt as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

    /// Generate a random unicast and locally administered MAC address.
    pub fn generate_rand() -> Self {
        let mut mac = rand::rng().random::<[u8; 6]>();

        // Set locally administered addresses bit and reset multicast bit.
        mac[0] = (mac[0] | 0x02) & 0xfe;

        Self(mac)
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

impl Serialize for MacAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = self
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":");

        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for MacAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        let bytes: Result<Vec<u8>, _> = s
            .split(':')
            .map(|part| {
                if part.len() != 2 {
                    return Err("each MAC address segment must contain 2 hex digits");
                }

                u8::from_str_radix(part, 16).map_err(|_| "invalid hexadecimal byte")
            })
            .collect();

        let bytes = bytes.map_err(serde::de::Error::custom)?;

        let bytes: [u8; 6] = bytes.try_into().map_err(|v: Vec<u8>| {
            serde::de::Error::custom(format!("expected 6 MAC address bytes, got {}", v.len()))
        })?;

        Ok(MacAddress(bytes))
    }
}
