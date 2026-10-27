//! Telling the local network from the Internet by the far end's address.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Where a connection's far end is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// This computer: counted nowhere, as the adaptors don't carry it.
    Loopback,
    Lan,
    Internet,
}

/// A network this computer is on, from its own addresses: IPv6 homes often
/// talk to each other with global addresses from the same prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefix {
    pub addr: IpAddr,
    pub len: u8,
}

impl Prefix {
    fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                let len = self.len.min(32) as u32;
                let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
                u32::from(a) & mask == u32::from(b) & mask
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                let len = self.len.min(128) as u32;
                let mask = if len == 0 {
                    0
                } else {
                    u128::MAX << (128 - len)
                };
                u128::from(a) & mask == u128::from(b) & mask
            }
            _ => false,
        }
    }
}

fn v4(ip: Ipv4Addr) -> Option<Place> {
    if ip.is_loopback() || ip.is_unspecified() {
        Some(Place::Loopback)
    } else if ip.is_private() || ip.is_link_local() || ip.is_broadcast() || ip.is_multicast() {
        Some(Place::Lan)
    } else {
        None
    }
}

fn v6(ip: Ipv6Addr) -> Option<Place> {
    if let Some(m) = ip.to_ipv4_mapped() {
        return v4(m);
    }
    let first = ip.segments()[0];
    if ip.is_loopback() || ip.is_unspecified() {
        Some(Place::Loopback)
    } else if first & 0xffc0 == 0xfe80 // link-local
        || first & 0xfe00 == 0xfc00 // unique local
        || ip.is_multicast()
    {
        Some(Place::Lan)
    } else {
        None
    }
}

/// Where `ip` is, given the networks this computer is on. Its own
/// addresses count as loopback: the system carries that traffic on `lo`.
pub fn place(ip: IpAddr, local: &[Prefix]) -> Place {
    if local.iter().any(|p| p.addr == ip) {
        return Place::Loopback;
    }
    let by_kind = match ip {
        IpAddr::V4(a) => v4(a),
        IpAddr::V6(a) => v6(a),
    };
    by_kind.unwrap_or_else(|| {
        if local.iter().any(|p| p.contains(ip)) {
            Place::Lan
        } else {
            Place::Internet
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_ranges_are_lan() {
        for a in [
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.1",
            "192.168.1.20",
            "169.254.3.4",
            "fe80::1",
            "fd12:3456::1",
            "::ffff:192.168.0.5",
        ] {
            assert_eq!(place(ip(a), &[]), Place::Lan, "{a}");
        }
    }

    #[test]
    fn public_is_internet() {
        for a in [
            "8.8.8.8",
            "172.32.0.1",
            "2a00:1450:4009::200e",
            "::ffff:1.1.1.1",
        ] {
            assert_eq!(place(ip(a), &[]), Place::Internet, "{a}");
        }
    }

    #[test]
    fn loopback() {
        assert_eq!(place(ip("127.0.0.1"), &[]), Place::Loopback);
        assert_eq!(place(ip("::1"), &[]), Place::Loopback);
    }

    #[test]
    fn our_own_prefix_is_lan() {
        let local = [Prefix {
            addr: ip("2a02:8010:1234:5600::17"),
            len: 64,
        }];
        assert_eq!(place(ip("2a02:8010:1234:5600::99"), &local), Place::Lan);
        assert_eq!(
            place(ip("2a02:8010:1234:5600::17"), &local),
            Place::Loopback
        );
        assert_eq!(
            place(ip("2a02:8010:1234:5601::99"), &local),
            Place::Internet
        );
    }
}
