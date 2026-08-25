use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use ipnet::IpNet;

use crate::{IpamError, POOL_DEFAULT, Res, bitmap::AllocationBitmap};

pub struct HostScopeAllocator {
    net: IpNet,
    base: u128,
    max: usize,
    alloc: AllocationBitmap,
}

impl HostScopeAllocator {
    pub fn new(cidr: IpNet) -> Self {
        let mut base = ip_to_u128(cidr.network());
        let size = Self::size(cidr);

        let max = if size > 2 { size - 2 } else { size };
        if size > 2 {
            base += 1;
        }
        let max = max as usize;

        Self {
            net: cidr,
            base,
            max,
            alloc: AllocationBitmap::new(max),
        }
    }

    fn size(n: IpNet) -> u64 {
        let ones = u32::from(n.prefix_len());
        let bits = u32::from(n.max_prefix_len());
        let host_bits = bits - ones;

        if (bits == 32 && host_bits >= 31) || (bits == 128 && host_bits >= 127) {
            return 0;
        }

        if bits == 128 && host_bits >= 16 {
            1 << 16
        } else {
            1 << host_bits
        }
    }

    pub fn allocate(&self, ip: IpAddr) -> Res<IpAddr> {
        let offset = self.contains(ip).ok_or_else(|| IpamError::NotInRange {
            valid_range: self.net.to_string(),
        })?;
        if !self.alloc.allocate(offset) {
            return Err(IpamError::AlreadyAllocated);
        }
        Ok(ip)
    }

    pub fn allocate_next(&self) -> Res<IpAddr> {
        let offset = self.alloc.allocate_next().ok_or(IpamError::RangeFull)?;
        let ip = offset_to_ip(self.base, offset, self.net);
        Ok(ip)
    }

    pub fn release(&self, ip: IpAddr) {
        if let Some(offset) = self.contains(ip) {
            self.alloc.release(offset);
        }
    }

    pub fn for_each(&self, mut f: impl FnMut(IpAddr)) {
        self.alloc
            .for_each(|offset| f(offset_to_ip(self.base, offset, self.net)));
    }

    pub fn dump(&self) -> (HashMap<String, HashMap<String, String>>, String) {
        let mut alloc = HashMap::new();
        self.for_each(|ip| {
            alloc.insert(ip.to_string(), String::new());
        });
        let max_ips = count_ips_in_cidr(self.net);
        let status = format!("{}/{} allocated from {}", alloc.len(), max_ips, self.net);
        (HashMap::from([(POOL_DEFAULT.to_string(), alloc)]), status)
    }

    fn contains(&self, ip: IpAddr) -> Option<usize> {
        if !self.net.contains(&ip) {
            return None;
        }
        let offset = ip_to_u128(ip).checked_sub(self.base)?;
        let offset = usize::try_from(offset).ok()?;
        (offset < self.max).then_some(offset)
    }

    #[cfg(test)]
    fn capacity(&self) -> u64 {
        count_ips_in_cidr(self.net) as u64
    }

    #[cfg(test)]
    pub fn has(&self, ip: IpAddr) -> bool {
        match self.contains(ip) {
            Some(offset) => self.alloc.has(offset),
            None => false,
        }
    }

    #[cfg(test)]
    pub fn used(&self) -> usize {
        self.max - self.free()
    }

    #[cfg(test)]
    pub fn free(&self) -> usize {
        self.alloc.free()
    }
}

fn ip_to_u128(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(v4) => u128::from(u32::from(v4)),
        IpAddr::V6(v6) => u128::from(v6),
    }
}

fn offset_to_ip(base: u128, offset: usize, net: IpNet) -> IpAddr {
    let value = base + offset as u128;
    match net {
        IpNet::V4(_) => IpAddr::V4(Ipv4Addr::from(value as u32)),
        IpNet::V6(_) => IpAddr::V6(Ipv6Addr::from(value)),
    }
}

fn count_ips_in_cidr(n: IpNet) -> u128 {
    let ones = u32::from(n.prefix_len());
    let bits = u32::from(n.max_prefix_len());
    if ones == bits {
        return 0;
    }
    (1u128 << (bits - ones)) - 2
}

#[cfg(test)]
mod tests {
    use ipnet::{Ipv4Net, Ipv6Net};

    use super::*;
    use crate::IpamError;

    fn cidr(s: &str) -> IpNet {
        s.parse().unwrap()
    }

    fn v4(prefix_len: u8) -> IpNet {
        IpNet::V4(Ipv4Net::new(Ipv4Addr::UNSPECIFIED, prefix_len).unwrap())
    }

    fn v6(prefix_len: u8) -> IpNet {
        IpNet::V6(Ipv6Net::new(Ipv6Addr::UNSPECIFIED, prefix_len).unwrap())
    }

    #[test]
    fn ipv4_typical_prefixes() {
        assert_eq!(HostScopeAllocator::size(v4(24)), 256);
        assert_eq!(HostScopeAllocator::size(v4(16)), 65536);
        assert_eq!(HostScopeAllocator::size(v4(8)), 16_777_216);
        assert_eq!(HostScopeAllocator::size(v4(32)), 1);
        assert_eq!(HostScopeAllocator::size(v4(30)), 4);
    }

    #[test]
    fn ipv4_smallest_ranges_are_zero() {
        assert_eq!(HostScopeAllocator::size(v4(1)), 0);
        assert_eq!(HostScopeAllocator::size(v4(0)), 0);
    }

    #[test]
    fn ipv6_is_capped_at_65536() {
        assert_eq!(HostScopeAllocator::size(v6(112)), 65536);
        assert_eq!(HostScopeAllocator::size(v6(64)), 65536);
        assert_eq!(HostScopeAllocator::size(v6(128)), 1);
    }

    #[test]
    fn ipv6_smallest_ranges_are_zero() {
        assert_eq!(HostScopeAllocator::size(v6(1)), 0);
        assert_eq!(HostScopeAllocator::size(v6(0)), 0);
    }

    #[test]
    fn new_excludes_network_and_broadcast_for_slash_24() {
        let r = HostScopeAllocator::new(cidr("10.0.0.0/24"));
        assert_eq!(r.free(), 254);
        assert_eq!(r.used(), 0);
        assert!(!r.has("10.0.0.0".parse().unwrap())); // network addr
        assert!(!r.has("10.0.0.255".parse().unwrap())); // broadcast addr
        assert!(
            r.allocate("10.0.0.0".parse().unwrap()).is_err(),
            "network address should be out of range"
        );
        assert!(
            r.allocate("10.0.0.255".parse().unwrap()).is_err(),
            "broadcast address should be out of range"
        );
    }

    #[test]
    fn slash_31_keeps_both_addresses() {
        // size() == 2 here, so the "-2 for network/broadcast" adjustment
        // in `new` is skipped -- matches the Go original's `if size > 2`.
        let r = HostScopeAllocator::new(cidr("10.0.0.0/31"));
        assert_eq!(r.free(), 2);
        assert!(r.allocate("10.0.0.0".parse().unwrap()).is_ok());
        assert!(r.allocate("10.0.0.1".parse().unwrap()).is_ok());
    }

    #[test]
    fn allocate_specific_ip_then_reject_reallocation() {
        let r = HostScopeAllocator::new(cidr("10.0.0.0/24"));
        let ip: IpAddr = "10.0.0.5".parse().unwrap();

        assert!(r.allocate(ip).is_ok());
        assert!(r.has(ip));
        assert_eq!(r.used(), 1);
        assert_eq!(r.allocate(ip), Err(IpamError::AlreadyAllocated));
    }

    #[test]
    fn allocate_out_of_range_ip_fails() {
        let r = HostScopeAllocator::new(cidr("10.0.0.0/24"));
        let outside: IpAddr = "10.0.1.5".parse().unwrap();
        assert!(matches!(
            r.allocate(outside),
            Err(IpamError::NotInRange { .. })
        ));
    }

    #[test]
    fn allocate_next_returns_addresses_in_cidr_and_reports_full() {
        let r = HostScopeAllocator::new(cidr("10.0.0.0/30")); // size 4, max 2 usable
        let a = r.allocate_next().unwrap();
        let b = r.allocate_next().unwrap();
        assert_ne!(a, b);
        assert!(r.net.contains(&a) && r.net.contains(&b));
        assert_eq!(r.allocate_next(), Err(IpamError::RangeFull));
    }

    #[test]
    fn release_then_reuse() {
        let r = HostScopeAllocator::new(cidr("10.0.0.0/30"));
        let ip = r.allocate_next().unwrap();
        r.release(ip);
        assert_eq!(r.free(), 2);
        assert!(!r.has(ip));
    }

    #[test]
    fn release_unallocated_ip_is_a_noop() {
        let r = HostScopeAllocator::new(cidr("10.0.0.0/24"));
        r.release("10.0.0.5".parse().unwrap()); // never allocated
        assert_eq!(r.used(), 0);
    }

    #[test]
    fn for_each_visits_all_allocated_ips() {
        let r = HostScopeAllocator::new(cidr("10.0.0.0/29")); // max 6
        let a = r.allocate_next().unwrap();
        let b = r.allocate_next().unwrap();

        let mut seen = Vec::new();
        r.for_each(|ip| seen.push(ip));
        seen.sort();
        let mut want = [a, b];
        want.sort();
        assert_eq!(seen, want);
    }

    #[test]
    fn ipv6_range_allocates() {
        let r = HostScopeAllocator::new(cidr("fd00::/120")); // size 256
        let a = r.allocate_next().unwrap();
        assert!(matches!(a, IpAddr::V6(_)));
        assert!(r.has(a));
    }

    #[test]
    fn new_cidr_range_base_and_max_match_reference_cases() {
        let cases: &[(&str, &str, usize)] = &[
            ("192.168.0.1/27", "192.168.0.1", 30),
            ("192.168.0.1/31", "192.168.0.0", 2),
            ("192.168.0.1/32", "192.168.0.1", 1),
            ("2001:db8::1/64", "2001:db8::1", 65534),
            ("2001:db8::1/120", "2001:db8::1", 254),
            ("2001:db8::1/127", "2001:db8::0", 2),
            ("2001:db8::1/128", "2001:db8::1", 1),
        ];

        for (net, want_base, want_max) in cases {
            let r = HostScopeAllocator::new(cidr(net));
            let base_ip = offset_to_ip(r.base, 0, r.net);
            assert_eq!(
                base_ip,
                want_base.parse::<IpAddr>().unwrap(),
                "base for {net}"
            );
            assert_eq!(r.max, *want_max, "max for {net}");
        }
    }

    #[test]
    fn range_size_reference_cases() {
        let cases: &[(&str, u64)] = &[
            ("192.168.0.0/27", 32),
            ("192.168.0.0/32", 1),
            ("2001:db8::/64", 65536),
            ("2001:db8::/120", 256),
            ("2001:db8::/128", 1),
        ];

        for (net, want) in cases {
            assert_eq!(HostScopeAllocator::size(cidr(net)), *want, "size for {net}");
        }
    }

    #[test]
    fn allocate_specific_ip() {
        let a = HostScopeAllocator::new(cidr("10.0.0.0/24"));
        let ip: IpAddr = "10.0.0.5".parse().unwrap();

        let result = a.allocate(ip).unwrap();
        assert_eq!(result, ip);
        assert_eq!(a.allocate(ip), Err(IpamError::AlreadyAllocated));
    }

    #[test]
    fn allocate_next_then_release_then_reuse() {
        let a = HostScopeAllocator::new(cidr("10.0.0.0/30")); // 2 usable

        let first = a.allocate_next().unwrap();
        let second = a.allocate_next().unwrap();
        assert_ne!(first, second);
        assert_eq!(a.allocate_next(), Err(IpamError::RangeFull));

        a.release(first);
        let reused = a.allocate_next().unwrap();
        assert_eq!(reused, first);
    }

    #[test]
    fn allocate_without_sync_upstream_behaves_like_allocate() {
        let a = HostScopeAllocator::new(cidr("10.0.0.0/24"));
        let ip: IpAddr = "10.0.0.5".parse().unwrap();

        let result = a.allocate(ip).unwrap();
        assert_eq!(result, ip);
    }

    #[test]
    fn allocate_next_without_sync_upstream_behaves_like_allocate_next() {
        let a = HostScopeAllocator::new(cidr("10.0.0.0/30"));
        let result = a.allocate_next().unwrap();
        assert!(a.has(result));
    }

    #[test]
    fn dump_reports_allocated_ips_under_the_default_pool() {
        let a = HostScopeAllocator::new(cidr("10.0.0.0/29")); // 6 usable
        let first = a.allocate_next().unwrap();
        let second = a.allocate_next().unwrap();

        let (by_pool, status) = a.dump();
        let default_pool_alloc = &by_pool[POOL_DEFAULT];
        assert_eq!(default_pool_alloc.len(), 2);
        assert!(default_pool_alloc.contains_key(&first.to_string()));
        assert!(default_pool_alloc.contains_key(&second.to_string()));
        assert_eq!(status, "2/6 allocated from 10.0.0.0/29");
    }

    #[test]
    fn count_ips_in_cidr_reference_cases() {
        let cases: &[(&str, u128)] = &[
            ("10.0.0.0/24", 254),
            ("10.0.0.0/29", 6),
            ("10.0.0.0/31", 0), // 2 total addresses, both excluded
            ("10.0.0.0/32", 0), // no room to exclude 2 from 1
            ("2001:db8::/64", 18_446_744_073_709_551_614),
            ("2001:db8::/120", 254),
            ("2001:db8::/128", 0),
        ];

        for (net, want) in cases {
            assert_eq!(
                count_ips_in_cidr(cidr(net)),
                *want,
                "count_ips_in_cidr for {net}"
            );
        }
    }

    #[test]
    fn capacity_matches_the_allocators_usable_size_for_typical_ipv4_cidrs() {
        let a = HostScopeAllocator::new(cidr("10.0.0.0/24"));
        assert_eq!(a.capacity(), 254);
        assert_eq!(a.free() as u64, a.capacity());
    }

    #[test]
    fn capacity_uncapped_diverges_from_the_allocators_65536_ipv6_bitmap_cap() {
        let a = HostScopeAllocator::new(cidr("2001:db8::/64"));
        assert_eq!(a.free(), 65534);
        assert_eq!(a.capacity(), 18_446_744_073_709_551_614);
    }

    #[test]
    fn capacity_is_zero_for_a_single_address_cidr_even_though_its_allocatable() {
        let a = HostScopeAllocator::new(cidr("10.0.0.0/32"));
        assert_eq!(a.capacity(), 0);
        assert_eq!(a.free(), 1);
    }
}
