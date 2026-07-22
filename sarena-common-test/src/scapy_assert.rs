#[cfg(feature = "std")]
use serde::ser::{Serialize, SerializeStruct, Serializer};

pub const SCAPY_MAX_BUF: usize = 1518;
pub const SCAPY_MAX_ASSERTS: usize = 256;
pub const SCAPY_MAX_STR_LEN: usize = 128;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ScapyAssert {
    pub name: [u8; SCAPY_MAX_STR_LEN],
    pub file: [u8; SCAPY_MAX_STR_LEN],
    pub line: u32,
    pub first_layer: [u8; SCAPY_MAX_STR_LEN],
    pub expected_len: usize,
    pub expected_buf: [u8; SCAPY_MAX_BUF],
    pub actual_len: usize,
    pub actual_buf: [u8; SCAPY_MAX_BUF],
}

#[cfg(feature = "std")]
unsafe impl aya::Pod for ScapyAssert {}

#[cfg(feature = "std")]
impl Serialize for ScapyAssert {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ScapyAssert", 8)?;
        state.serialize_field("name", &convert::<S>(&self.name)?)?;
        state.serialize_field("file", &convert::<S>(&self.file)?)?;
        state.serialize_field("linenum", &self.line)?;
        state.serialize_field("first-layer", &convert::<S>(&self.first_layer)?)?;
        state.serialize_field("exp-len", &self.expected_len)?;
        state.serialize_field(
            "exp-buf",
            &encode_to_string(&self.expected_buf[..self.expected_len]),
        )?;
        state.serialize_field("got-len", &self.actual_len)?;
        state.serialize_field(
            "got-buf",
            &encode_to_string(&self.actual_buf[..self.actual_len]),
        )?;
        state.end()
    }
}

impl ScapyAssert {
    pub const fn null() -> Self {
        Self {
            name: [0; SCAPY_MAX_STR_LEN],
            file: [0; SCAPY_MAX_STR_LEN],
            line: 0,
            first_layer: [0; SCAPY_MAX_STR_LEN],
            expected_len: 0,
            expected_buf: [0; SCAPY_MAX_BUF],
            actual_len: 0,
            actual_buf: [0; SCAPY_MAX_BUF],
        }
    }
}

pub static SCAPY_ASSERT_NULL: ScapyAssert = ScapyAssert::null();

#[cfg(feature = "std")]
fn encode_to_string(bytes: &[u8]) -> String {
    let mut out = vec![0u8; bytes.len() * 2];
    hex::encode_to_slice(bytes, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[cfg(feature = "std")]
fn convert<S>(bytes: &[u8; SCAPY_MAX_STR_LEN]) -> Result<String, S::Error>
where
    S: Serializer,
{
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let x = str::from_utf8(&bytes[..len]).map_err(serde::ser::Error::custom)?;
    Ok(x.to_string())
}
