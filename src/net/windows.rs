//! Windows: adaptors from the IP Helper's interface table.
//!
//! Telling the LAN from the Internet needs each connection's byte counts,
//! which Windows only collects for administrators (TCP extended
//! statistics), so on Windows the meter shows all traffic for now.

use super::Adaptor;
use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

/// IF_TYPE_SOFTWARE_LOOPBACK
const LOOPBACK: u32 = 24;

pub fn adaptors() -> std::io::Result<Vec<Adaptor>> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table, which is read here and freed
    // once with FreeMibTable.
    let rows = unsafe {
        let err = GetIfTable2(&mut table);
        if err != 0 || table.is_null() {
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        let mut v: Vec<Adaptor> = Vec::new();
        for r in rows {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 1 != 0;
            let filter = flags & 2 != 0;
            // Filter rows (WFP, QoS, …) repeat the adaptor they sit on.
            if filter || r.Type == LOOPBACK {
                continue;
            }
            let end = r
                .Alias
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(r.Alias.len());
            let alias = String::from_utf16_lossy(&r.Alias[..end]);
            let mut id = alias.clone();
            let mut k = 2;
            while v.iter().any(|a| a.id == id) {
                id = format!("{alias} ({k})");
                k += 1;
            }
            v.push(Adaptor {
                label: id.clone(),
                id,
                physical: hardware,
                rx: r.InOctets,
                tx: r.OutOctets,
            });
        }
        FreeMibTable(table.cast());
        v
    };
    Ok(rows)
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
