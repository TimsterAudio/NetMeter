//! Linux: adaptors from `/proc/net/dev`, and the LAN's share from the
//! kernel's TCP connection list (sock_diag over netlink, which needs no
//! special rights).
//!
//! The LAN figure is TCP only: file shares, media servers, backups and
//! copies between computers all use it. Local UDP (game streaming, some
//! casting) counts as Internet.

use super::Adaptor;
use crate::lan::{self, Place, Prefix};
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub fn adaptors() -> io::Result<Vec<Adaptor>> {
    let text = std::fs::read_to_string("/proc/net/dev")?;
    Ok(parse_proc_net_dev(&text)
        .into_iter()
        .map(|(name, rx, tx)| {
            let physical = name != "lo" && !is_virtual(&name);
            Adaptor {
                label: name.clone(),
                id: name,
                physical,
                rx,
                tx,
            }
        })
        .collect())
}

/// Bridges, tunnels, VPNs and containers' links live under
/// /sys/devices/virtual.
fn is_virtual(name: &str) -> bool {
    std::fs::canonicalize(format!("/sys/class/net/{name}"))
        .map(|p| p.to_string_lossy().contains("/virtual/"))
        .unwrap_or(false)
}

/// (name, bytes received, bytes sent) for each line of /proc/net/dev.
fn parse_proc_net_dev(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let f: Vec<u64> = rest
                .split_whitespace()
                .map(|x| x.parse().ok())
                .collect::<Option<_>>()?;
            if f.len() < 9 {
                return None;
            }
            Some((name.trim().to_string(), f[0], f[8]))
        })
        .collect()
}

// ---------------------------------------------------------------- sock_diag

const NETLINK_SOCK_DIAG: i32 = 4;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 1;
const NLM_F_DUMP: u16 = 0x300;
const INET_DIAG_INFO: u16 = 2;
const NLMSG_HDR: usize = 16;
const DIAG_MSG: usize = 72;

/// What the kernel says about one TCP connection.
#[derive(Clone, Copy, Debug)]
struct TcpSock {
    cookie: u64,
    /// For the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    local_port: u16,
    remote: IpAddr,
    /// Payload the far end has acknowledged, and payload received.
    acked: u64,
    received: u64,
    /// Segments out and in (kernels from 4.2), for the headers' share.
    segs: Option<(u32, u32)>,
}

impl TcpSock {
    /// Bytes on the wire, received and sent: the payload plus Ethernet, IP
    /// and TCP headers (with timestamps) on each segment, as the adaptor
    /// counts them.
    fn wire(&self) -> (u64, u64) {
        let hdr: u64 = if self.remote.is_ipv4() { 66 } else { 86 };
        match self.segs {
            Some((out, inn)) => (
                self.received + inn as u64 * hdr,
                self.acked + out as u64 * hdr,
            ),
            // Older kernels: about one header per full segment, and an
            // acknowledgement back for every two.
            None => {
                let (r, a) = (self.received as f64, self.acked as f64);
                (
                    (r * 1.046 + a * 0.023) as u64,
                    (a * 1.046 + r * 0.023) as u64,
                )
            }
        }
    }
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_ne_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_ne_bytes(b[i..i + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_ne_bytes(b[i..i + 8].try_into().unwrap())
}

struct Diag {
    fd: OwnedFd,
    buf: Vec<u8>,
}

impl Diag {
    fn open() -> io::Result<Diag> {
        // SAFETY: a plain socket call; the descriptor is owned from here on.
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
                NETLINK_SOCK_DIAG,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Diag {
            // SAFETY: `fd` was just opened and nothing else owns it.
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            buf: vec![0; 64 * 1024],
        })
    }

    /// Every TCP connection of one address family.
    fn dump(&mut self, family: u8, out: &mut Vec<TcpSock>) -> io::Result<()> {
        let mut req = [0u8; NLMSG_HDR + 56];
        let len = req.len() as u32;
        req[0..4].copy_from_slice(&len.to_ne_bytes());
        req[4..6].copy_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
        req[6..8].copy_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
        // inet_diag_req_v2: family, protocol, extensions wanted, pad, states.
        req[16] = family;
        req[17] = libc::IPPROTO_TCP as u8;
        req[18] = 1 << (INET_DIAG_INFO - 1);
        req[20..24].copy_from_slice(&u32::MAX.to_ne_bytes());
        // SAFETY: `addr` is a zeroed sockaddr_nl with its family set, and
        // `req` outlives the call.
        let sent = unsafe {
            let mut addr: libc::sockaddr_nl = std::mem::zeroed();
            addr.nl_family = libc::AF_NETLINK as u16;
            libc::sendto(
                self.fd.as_raw_fd(),
                req.as_ptr().cast(),
                req.len(),
                0,
                (&addr as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as u32,
            )
        };
        if sent < 0 {
            return Err(io::Error::last_os_error());
        }
        loop {
            // SAFETY: reads into our own buffer, up to its length.
            let n = unsafe {
                libc::recv(
                    self.fd.as_raw_fd(),
                    self.buf.as_mut_ptr().cast(),
                    self.buf.len(),
                    0,
                )
            };
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            let b = &self.buf[..n as usize];
            let mut i = 0;
            while i + NLMSG_HDR <= b.len() {
                let len = u32_at(b, i) as usize;
                let kind = u16_at(b, i + 4);
                if len < NLMSG_HDR || i + len > b.len() {
                    return Ok(());
                }
                match kind {
                    NLMSG_DONE => return Ok(()),
                    NLMSG_ERROR => {
                        let code = if len >= NLMSG_HDR + 4 {
                            -(u32_at(b, i + NLMSG_HDR) as i32)
                        } else {
                            libc::EIO
                        };
                        return Err(io::Error::from_raw_os_error(code));
                    }
                    SOCK_DIAG_BY_FAMILY => {
                        if let Some(s) = parse_diag(&b[i + NLMSG_HDR..i + len]) {
                            out.push(s);
                        }
                    }
                    _ => {}
                }
                i += (len + 3) & !3;
            }
        }
    }
}

/// One inet_diag_msg and its attributes.
fn parse_diag(m: &[u8]) -> Option<TcpSock> {
    if m.len() < DIAG_MSG {
        return None;
    }
    let family = m[0];
    // inet_diag_sockid from byte 4: ports (big-endian), addresses,
    // interface, cookie.
    let local_port = u16::from_be_bytes([m[4], m[5]]);
    let dst = &m[24..40];
    let remote = if family == libc::AF_INET as u8 {
        IpAddr::V4(Ipv4Addr::new(dst[0], dst[1], dst[2], dst[3]))
    } else {
        IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(dst).ok()?))
    };
    let cookie = u32_at(m, 44) as u64 | (u32_at(m, 48) as u64) << 32;
    let mut i = DIAG_MSG;
    while i + 4 <= m.len() {
        let len = u16_at(m, i) as usize;
        let kind = u16_at(m, i + 2);
        if len < 4 || i + len > m.len() {
            break;
        }
        if kind == INET_DIAG_INFO {
            // struct tcp_info: bytes_acked at 120, bytes_received at 128,
            // segs_out and segs_in at 136 and 140.
            let info = &m[i + 4..i + len];
            if info.len() < 136 {
                return None;
            }
            let segs = (info.len() >= 144).then(|| (u32_at(info, 136), u32_at(info, 140)));
            return Some(TcpSock {
                cookie,
                local_port,
                remote,
                acked: u64_at(info, 120),
                received: u64_at(info, 128),
                segs,
            });
        }
        i += (len + 3) & !3;
    }
    None
}

/// Every TCP connection on the computer, IPv4 and IPv6.
#[cfg(test)]
fn tcp_sockets() -> io::Result<Vec<TcpSock>> {
    let mut d = Diag::open()?;
    let mut v = Vec::new();
    d.dump(libc::AF_INET as u8, &mut v)?;
    d.dump(libc::AF_INET6 as u8, &mut v)?;
    Ok(v)
}

/// The networks this computer's own addresses are on.
fn local_prefixes() -> Vec<Prefix> {
    let mut v = Vec::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `head` with a list we walk read-only and
    // free once.
    unsafe {
        if libc::getifaddrs(&mut head) != 0 {
            return v;
        }
        let mut p = head;
        while !p.is_null() {
            let ifa = &*p;
            p = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || ifa.ifa_netmask.is_null() {
                continue;
            }
            match (*ifa.ifa_addr).sa_family as i32 {
                libc::AF_INET => {
                    let a = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let m = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                    let len = u32::from_be(m.sin_addr.s_addr).leading_ones() as u8;
                    if len >= 8 {
                        v.push(Prefix {
                            addr: IpAddr::V4(Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr))),
                            len,
                        });
                    }
                }
                libc::AF_INET6 => {
                    let a = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    let m = &*(ifa.ifa_netmask as *const libc::sockaddr_in6);
                    let len = u128::from_be_bytes(m.sin6_addr.s6_addr).leading_ones() as u8;
                    if len >= 32 {
                        v.push(Prefix {
                            addr: IpAddr::V6(Ipv6Addr::from(a.sin6_addr.s6_addr)),
                            len,
                        });
                    }
                }
                _ => {}
            }
        }
        libc::freeifaddrs(head);
    }
    v
}

/// Counts the bytes to and from the local network between reads.
pub struct LanMeter {
    diag: Diag,
    /// Each LAN connection's bytes on the wire at the last read.
    last: HashMap<u64, (u64, u64)>,
    primed: bool,
    prefixes: Vec<Prefix>,
    reads: u32,
}

impl LanMeter {
    pub fn new() -> Option<LanMeter> {
        let mut m = LanMeter {
            diag: Diag::open().ok()?,
            last: HashMap::new(),
            primed: false,
            prefixes: Vec::new(),
            reads: 0,
        };
        m.read()?;
        Some(m)
    }

    /// Bytes received and sent on the LAN since the last read. Connections
    /// that closed since then lose their last moments to the Internet's
    /// figure.
    pub fn read(&mut self) -> Option<(u64, u64)> {
        if self.reads.is_multiple_of(20) {
            self.prefixes = local_prefixes();
        }
        self.reads = self.reads.wrapping_add(1);
        let mut socks = Vec::new();
        self.diag.dump(libc::AF_INET as u8, &mut socks).ok()?;
        self.diag.dump(libc::AF_INET6 as u8, &mut socks).ok()?;
        let mut now = HashMap::with_capacity(self.last.len());
        let (mut rx, mut tx) = (0u64, 0u64);
        for s in socks {
            if lan::place(s.remote, &self.prefixes) != Place::Lan {
                continue;
            }
            let (r, t) = s.wire();
            match self.last.get(&s.cookie) {
                Some(&(r0, t0)) => {
                    rx += r.saturating_sub(r0);
                    tx += t.saturating_sub(t0);
                }
                // A connection opened since the last read: all of it is new.
                None if self.primed => {
                    rx += r;
                    tx += t;
                }
                None => {}
            }
            now.insert(s.cookie, (r, t));
        }
        self.last = now;
        let first = !self.primed;
        self.primed = true;
        Some(if first { (0, 0) } else { (rx, tx) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn reads_proc_net_dev() {
        let text = "Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo:  123456     100    0    0    0     0          0         0   123456     100    0    0    0     0       0          0
  eth0: 9876543210 5000    0    0    0     0          0        12 1234567     4000    0    0    0     0       0          0
";
        let v = parse_proc_net_dev(text);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1], ("eth0".to_string(), 9_876_543_210, 1_234_567));
    }

    #[test]
    fn sees_a_connection_and_its_bytes() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let data = vec![7u8; 300_000];
        client.write_all(&data).unwrap();
        let mut got = vec![0u8; data.len()];
        server.read_exact(&mut got).unwrap();
        let socks = match tcp_sockets() {
            Ok(s) => s,
            // Some sandboxes have no netlink; nothing to check there.
            Err(e) => return eprintln!("no sock_diag here: {e}"),
        };
        let srv = socks
            .iter()
            .find(|s| s.local_port == port && s.remote.is_loopback())
            .expect("the server's side of the connection");
        assert_eq!(srv.received, data.len() as u64);
        let (rx, _) = srv.wire();
        assert!(rx > srv.received);
    }
}
