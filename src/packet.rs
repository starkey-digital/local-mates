use std::net::Ipv4Addr;

pub const HOST_IP: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 1);
pub const PREFIX: u8 = 24;
const SUBNET_BROADCAST: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 255);

/// Source and destination of an IPv4 packet; `None` for anything else (we only route IPv4).
pub fn ipv4_src_dst(pkt: &[u8]) -> Option<(Ipv4Addr, Ipv4Addr)> {
    if pkt.len() < 20 || pkt[0] >> 4 != 4 {
        return None;
    }
    let src: [u8; 4] = pkt[12..16].try_into().ok()?;
    let dst: [u8; 4] = pkt[16..20].try_into().ok()?;
    Some((src.into(), dst.into()))
}

/// Packets every peer should see: LAN game discovery lives here.
pub fn is_fanout(dst: Ipv4Addr) -> bool {
    dst.is_broadcast() || dst == SUBNET_BROADCAST || dst.is_multicast()
}

pub fn peer_ip(index: u8) -> Ipv4Addr {
    Ipv4Addr::new(10, 77, 0, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipv4(src: [u8; 4], dst: [u8; 4]) -> Vec<u8> {
        let mut pkt = vec![0x45; 20];
        pkt[12..16].copy_from_slice(&src);
        pkt[16..20].copy_from_slice(&dst);
        pkt
    }

    #[test]
    fn parses_ipv4() {
        let pkt = ipv4([10, 77, 0, 2], [10, 77, 0, 1]);
        assert_eq!(ipv4_src_dst(&pkt), Some((peer_ip(2), HOST_IP)));
    }

    #[test]
    fn rejects_ipv6_and_short() {
        let mut pkt = ipv4([0; 4], [0; 4]);
        pkt[0] = 0x60;
        assert_eq!(ipv4_src_dst(&pkt), None);
        assert_eq!(ipv4_src_dst(&[0x45; 10]), None);
    }

    #[test]
    fn fanout() {
        assert!(is_fanout(Ipv4Addr::BROADCAST));
        assert!(is_fanout(SUBNET_BROADCAST));
        assert!(is_fanout(Ipv4Addr::new(239, 255, 255, 250)));
        assert!(!is_fanout(peer_ip(3)));
    }
}
