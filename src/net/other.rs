//! Elsewhere: nothing to measure yet.

use super::Adaptor;

pub fn adaptors() -> std::io::Result<Vec<Adaptor>> {
    Err(std::io::Error::other("not supported on this system"))
}

pub struct LanMeter;

impl LanMeter {
    pub fn new() -> Option<LanMeter> {
        None
    }

    pub fn read(&mut self) -> Option<(u64, u64)> {
        None
    }
}
