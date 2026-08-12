#![cfg_attr(not(feature = "std"), no_std)]
#![no_builtins]

mod config;

pub use config::*;

pub type Ipv4Key = u32; // network byte order throughout

pub trait Ipv4KeyExt: Sized {
    /// The four wire octets, in the same order as
    /// [`core::net::Ipv4Addr::octets`]. Deliberately `to_ne_bytes`, not
    /// `to_be_bytes`: an `Ipv4Key` already stores the wire bytes verbatim,
    /// so on this target the native-endian bytes *are* the octets.
    fn octets(self) -> [u8; 4];

    /// The inverse of [`Ipv4KeyExt::octets`].
    fn from_octets(octets: [u8; 4]) -> Self;

    /// For logging/display -- e.g. `info!(&ctx, "ip: {:i}", key.to_addr())`.
    fn to_addr(self) -> core::net::Ipv4Addr {
        core::net::Ipv4Addr::from(self.octets())
    }

    /// Convenience mirror of [`Ipv4KeyExt::to_addr`].
    fn from_addr(addr: core::net::Ipv4Addr) -> Self {
        Self::from_octets(addr.octets())
    }
}

impl Ipv4KeyExt for Ipv4Key {
    fn octets(self) -> [u8; 4] {
        self.to_ne_bytes()
    }

    fn from_octets(octets: [u8; 4]) -> Self {
        Self::from_ne_bytes(octets)
    }
}
