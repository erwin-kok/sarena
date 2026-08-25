use std::{collections::HashMap, net::IpAddr, sync::RwLock};

use async_trait::async_trait;
use ipnet::IpNet;
use sarena_api_types_v1::ipam::{ContainerAddressing, HostAddressing, IpamAllocateResponse};
use tracing::info;

use crate::{IpamError, IpamService, POOL_DEFAULT, Res, host_scope::HostScopeAllocator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationResult {
    /// The allocated IP.
    pub ip: IpAddr,
    /// The IPAM pool the above IP was allocated from.
    pub ip_pool_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Ipv4,
    Ipv6,
}

struct State {
    ipv4_allocator: Option<HostScopeAllocator>,
    ipv6_allocator: Option<HostScopeAllocator>,
    owner: HashMap<String, HashMap<String, String>>,
    excluded_ips: HashMap<String, String>,
}

impl State {
    /// registers a new owner for an IP in a particular pool.
    fn register_ip_owner(&mut self, ip: IpAddr, owner: &str, pool: &str) {
        self.owner
            .entry(pool.to_string())
            .or_default()
            .insert(ip.to_string(), owner.to_string());
    }

    /// returns the owner for an IP in a particular pool or the empty string in case the pool or IP
    /// is not registered.
    fn get_ip_owner(&self, ip: &str, pool: &str) -> String {
        self.owner
            .get(pool)
            .and_then(|p| p.get(ip))
            .cloned()
            .unwrap_or_default()
    }

    /// releases ip from pool and returns the previous owner.
    fn release_ip_owner(&mut self, ip: IpAddr, pool: &str) -> String {
        let Some(m) = self.owner.get_mut(pool) else {
            return String::new();
        };

        let owner = m.remove(&ip.to_string()).unwrap_or_default();
        if m.is_empty() {
            self.owner.remove(pool);
        }
        owner
    }

    fn release_ip_locked(&mut self, ip: IpAddr, pool: &str) -> Res<()> {
        match ip {
            IpAddr::V4(_) => {
                let allocator = self
                    .ipv4_allocator
                    .as_ref()
                    .ok_or(IpamError::Ipv4Disabled)?;
                allocator.release(ip);
            }
            IpAddr::V6(_) => {
                let allocator = self
                    .ipv6_allocator
                    .as_ref()
                    .ok_or(IpamError::Ipv6Disabled)?;
                allocator.release(ip);
            }
        }

        let owner = self.release_ip_owner(ip, pool);
        info!("release ip: {ip}, owner: {owner}");

        Ok(())
    }
}

pub struct DefaultIpamService {
    state: RwLock<State>,
    gateway_ip: IpAddr,
}

#[async_trait]
impl IpamService for DefaultIpamService {
    async fn allocate(
        &self,
        family: Option<String>,
        owner: Option<String>,
        pool: Option<String>,
        _expiration: bool,
    ) -> Res<IpamAllocateResponse> {
        let owner = owner.unwrap_or_default();
        let pool = pool.unwrap_or_else(|| POOL_DEFAULT.to_string());
        let family = match family.as_deref() {
            Some("ipv4") => Some(Family::Ipv4),
            Some("ipv6") => Some(Family::Ipv6),
            _ => None,
        };

        let (ipv4, ipv6) = self.allocate_next(family, &owner, &pool)?;

        let response = IpamAllocateResponse {
            host_addressing: HostAddressing {
                ipv4: Some(self.gateway_ip.to_string()),
                ipv6: None,
            },
            ipv4: ipv4.map(|result| ContainerAddressing {
                ip: result.ip.to_string(),
                pool: Some(result.ip_pool_name),
            }),
            ipv6: ipv6.map(|result| ContainerAddressing {
                ip: result.ip.to_string(),
                pool: Some(result.ip_pool_name),
            }),
        };

        info!(?response, "allocate ip response");

        Ok(response)
    }

    async fn allocate_ip(
        &self,
        ip: IpAddr,
        owner: Option<String>,
        pool: Option<String>,
    ) -> Res<()> {
        let owner = owner.unwrap_or_default();
        let pool = pool.unwrap_or_else(|| POOL_DEFAULT.to_string());
        self.allocate_ip(ip, &owner, &pool)?;
        Ok(())
    }

    async fn release(&self, ip: IpAddr, pool: Option<String>) -> Res<()> {
        let pool = pool.unwrap_or_else(|| POOL_DEFAULT.to_string());
        self.release_ip(ip, &pool)
    }

    async fn dump(&self) -> Res<(HashMap<String, String>, HashMap<String, String>, String)> {
        let mut allocv4 = HashMap::new();
        let mut allocv6 = HashMap::new();
        let mut st4 = String::new();
        let mut st6 = String::new();

        let state = self.state.read().expect("ipam state lock poisoned");

        if let Some(allocator) = state.ipv4_allocator.as_ref() {
            let (alloc_per_pool, status) = allocator.dump();
            st4 = format!("IPv4: {status}");
            for (pool, alloc) in alloc_per_pool {
                for ip in alloc.keys() {
                    let owner = state.get_ip_owner(ip, &pool);
                    let ip_prefix = if pool == POOL_DEFAULT {
                        String::new()
                    } else {
                        format!("{pool}/")
                    };
                    allocv4.insert(format!("{ip_prefix}{ip}"), owner);
                }
            }
        }

        if let Some(allocator) = state.ipv6_allocator.as_ref() {
            let (alloc_per_pool, status) = allocator.dump();
            st6 = format!("IPv6: {status}");
            for (pool, alloc) in alloc_per_pool {
                for ip in alloc.keys() {
                    let owner = state.get_ip_owner(ip, &pool);
                    let ip_prefix = if pool == POOL_DEFAULT {
                        String::new()
                    } else {
                        format!("{pool}/")
                    };
                    allocv6.insert(format!("{ip_prefix}{ip}"), owner);
                }
            }
        }

        let mut status = format!("{st4}, {st6}");
        if status.is_empty() {
            status = "Not running".to_string();
        }

        Ok((allocv4, allocv6, status))
    }
}

impl DefaultIpamService {
    pub fn new(gateway_ip: IpAddr, ipv4: Option<IpNet>, ipv6: Option<IpNet>) -> Self {
        Self {
            state: RwLock::new(State {
                ipv4_allocator: ipv4.map(HostScopeAllocator::new),
                ipv6_allocator: ipv6.map(HostScopeAllocator::new),
                owner: HashMap::new(),
                excluded_ips: HashMap::new(),
            }),
            gateway_ip,
        }
    }

    pub fn exclude_ip(&self, ip: IpAddr, owner: &str, pool: &str) {
        let key = format!("{pool}:{ip}");
        let mut state = self.state.write().expect("ipam state lock poisoned");
        state.excluded_ips.insert(key, owner.to_string());
    }

    fn allocate_ip(&self, ip: IpAddr, owner: &str, pool: &str) -> Res<AllocationResult> {
        if pool.is_empty() {
            return Err(IpamError::PoolRequiredForAllocate(
                ip.to_string(),
                owner.to_string(),
            ));
        }

        let mut state = self.state.write().expect("ipam state lock poisoned");

        let key = format!("{pool}:{ip}");
        if let Some(owned_by) = state.excluded_ips.get(&key).cloned() {
            return Err(IpamError::ExcludedError(ip.to_string(), owned_by));
        }

        let ip = match ip {
            IpAddr::V4(_) => {
                let allocator = state
                    .ipv4_allocator
                    .as_ref()
                    .ok_or(IpamError::Ipv4Disabled)?;
                allocator.allocate(ip)?
            }
            IpAddr::V6(_) => {
                let allocator = state
                    .ipv6_allocator
                    .as_ref()
                    .ok_or(IpamError::Ipv6Disabled)?;
                allocator.allocate(ip)?
            }
        };

        info!(
            "allocated specific ip: {ip}, owner: {owner}, pool: {:?}",
            &POOL_DEFAULT
        );

        state.register_ip_owner(ip, owner, pool);

        Ok(AllocationResult {
            ip,
            ip_pool_name: POOL_DEFAULT.to_string(),
        })
    }

    fn allocate_next(
        &self,
        family: Option<Family>,
        owner: &str,
        pool: &str,
    ) -> Res<(Option<AllocationResult>, Option<AllocationResult>)> {
        let want_ipv6 = matches!(family, None | Some(Family::Ipv6));
        let want_ipv4 = matches!(family, None | Some(Family::Ipv4));

        let ipv6_configured = self
            .state
            .read()
            .expect("ipam state lock poisoned")
            .ipv6_allocator
            .is_some();
        let ipv4_configured = self
            .state
            .read()
            .expect("ipam state lock poisoned")
            .ipv4_allocator
            .is_some();

        let mut ipv6_result = None;
        if want_ipv6 && ipv6_configured {
            ipv6_result = Some(self.allocate_next_family(Family::Ipv6, owner, pool)?);
        }

        let mut ipv4_result = None;
        if want_ipv4 && ipv4_configured {
            ipv4_result = match self.allocate_next_family(Family::Ipv4, owner, pool) {
                Ok(result) => Some(result),
                Err(err) => {
                    if let Some(ipv6_result) = &ipv6_result {
                        let _ = self.release_ip(ipv6_result.ip, &ipv6_result.ip_pool_name);
                    }
                    return Err(err);
                }
            };
        }

        Ok((ipv4_result, ipv6_result))
    }

    fn release_ip(&self, ip: IpAddr, pool: &str) -> Res<()> {
        if pool.is_empty() {
            return Err(IpamError::PoolRequiredForRelease(ip.to_string()));
        }

        let mut state = self.state.write().expect("ipam state lock poisoned");
        state.release_ip_locked(ip, pool)
    }

    fn allocate_next_family(
        &self,
        family: Family,
        owner: &str,
        pool: &str,
    ) -> Res<AllocationResult> {
        if pool.is_empty() {
            return Err(IpamError::PoolDeterminationUnavailable(owner.to_string()));
        }

        let mut state = self.state.write().expect("ipam state lock poisoned");

        loop {
            let ip = {
                let allocator = match family {
                    Family::Ipv6 => state
                        .ipv6_allocator
                        .as_ref()
                        .ok_or(IpamError::Ipv6Disabled)?,
                    Family::Ipv4 => state
                        .ipv4_allocator
                        .as_ref()
                        .ok_or(IpamError::Ipv4Disabled)?,
                };
                allocator.allocate_next()?
            };

            let key = format!("{pool}:{ip}");
            if !state.excluded_ips.contains_key(&key) {
                state.register_ip_owner(ip, owner, pool);
                return Ok(AllocationResult {
                    ip,
                    ip_pool_name: POOL_DEFAULT.to_string(),
                });
            }

            // The allocated IP is excluded, do not use it. The excluded
            // IP is now allocated so it won't be allocated again on the
            // next iteration.
            state.register_ip_owner(ip, &format!("{owner} (excluded)"), pool);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    fn cidr(s: &str) -> IpNet {
        s.parse().unwrap()
    }

    fn gateway_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))
    }

    fn state_with_ipv4(net: &str) -> State {
        State {
            ipv4_allocator: Some(HostScopeAllocator::new(cidr(net))),
            ipv6_allocator: None,
            owner: HashMap::new(),
            excluded_ips: HashMap::new(),
        }
    }

    fn state_none() -> State {
        State {
            ipv4_allocator: None,
            ipv6_allocator: None,
            owner: HashMap::new(),
            excluded_ips: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn state_register_then_get_ip_owner() {
        let mut state = state_with_ipv4("10.0.0.0/29");
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        state.register_ip_owner(ip, "pod-a", "default");
        assert_eq!(state.get_ip_owner("10.0.0.1", "default"), "pod-a");
    }

    #[tokio::test]
    async fn state_get_ip_owner_unregistered_is_empty() {
        let state = state_with_ipv4("10.0.0.0/29");
        assert_eq!(state.get_ip_owner("10.0.0.1", "default"), "");
    }

    #[tokio::test]
    async fn state_release_ip_owner_removes_entry_and_returns_previous_owner() {
        let mut state = state_with_ipv4("10.0.0.0/29");
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        state.register_ip_owner(ip, "pod-a", "default");

        assert_eq!(state.release_ip_owner(ip, "default"), "pod-a");
        assert_eq!(state.get_ip_owner("10.0.0.1", "default"), "");
        assert!(!state.owner.contains_key("default"));
    }

    #[tokio::test]
    async fn state_release_ip_owner_unregistered_returns_empty_and_does_not_panic() {
        let mut state = state_with_ipv4("10.0.0.0/29");
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert_eq!(state.release_ip_owner(ip, "default"), "");
    }

    #[tokio::test]
    async fn state_release_ip_owner_keeps_pool_entry_if_other_ips_remain() {
        let mut state = state_with_ipv4("10.0.0.0/29");
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        state.register_ip_owner(a, "pod-a", "default");
        state.register_ip_owner(b, "pod-b", "default");

        state.release_ip_owner(a, "default");
        assert!(state.owner.contains_key("default"));
        assert_eq!(state.get_ip_owner("10.0.0.2", "default"), "pod-b");
    }

    #[tokio::test]
    async fn state_release_ip_locked_returns_disabled_error_when_family_unconfigured() {
        let mut state = state_none();
        assert_eq!(
            state.release_ip_locked("10.0.0.1".parse().unwrap(), "default"),
            Err(IpamError::Ipv4Disabled)
        );
        assert_eq!(
            state.release_ip_locked("fd00::1".parse().unwrap(), "default"),
            Err(IpamError::Ipv6Disabled)
        );
    }

    #[tokio::test]
    async fn state_release_ip_locked_frees_allocator_and_clears_owner() {
        let mut state = state_with_ipv4("10.0.0.0/29");
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        state.ipv4_allocator.as_ref().unwrap().allocate(ip).unwrap();
        state.register_ip_owner(ip, "pod-a", "default");

        state.release_ip_locked(ip, "default").unwrap();

        assert_eq!(state.get_ip_owner("10.0.0.1", "default"), "");
        state.ipv4_allocator.as_ref().unwrap().allocate(ip).unwrap();
    }

    #[tokio::test]
    async fn allocate_ip_specific_address_then_reject_reallocation() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();

        let result = svc.allocate_ip(ip, "pod-a", "default").unwrap();
        assert_eq!(result.ip, ip);
        assert_eq!(result.ip_pool_name, POOL_DEFAULT);
        assert_eq!(
            svc.allocate_ip(ip, "pod-b", "default"),
            Err(IpamError::AlreadyAllocated)
        );
    }

    #[tokio::test]
    async fn allocate_ip_requires_non_empty_pool() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert_eq!(
            svc.allocate_ip(ip, "pod-a", ""),
            Err(IpamError::PoolRequiredForAllocate(
                ip.to_string(),
                "pod-a".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn allocate_ip_ipv4_disabled_when_not_configured() {
        let svc = DefaultIpamService::new(gateway_ip(), None, Some(cidr("fd00::/125")));
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert_eq!(
            svc.allocate_ip(ip, "pod-a", "default"),
            Err(IpamError::Ipv4Disabled)
        );
    }

    #[tokio::test]
    async fn allocate_ip_ipv6_disabled_when_not_configured() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "fd00::1".parse().unwrap();
        assert_eq!(
            svc.allocate_ip(ip, "pod-a", "default"),
            Err(IpamError::Ipv6Disabled)
        );
    }

    #[tokio::test]
    async fn allocate_ip_excluded_ip_is_rejected() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        svc.exclude_ip(ip, "reserved-owner", "default");

        assert_eq!(
            svc.allocate_ip(ip, "pod-a", "default"),
            Err(IpamError::ExcludedError(
                ip.to_string(),
                "reserved-owner".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn allocate_next_defaults_to_both_families_when_none_requested() {
        let svc = DefaultIpamService::new(
            gateway_ip(),
            Some(cidr("10.0.0.0/29")),
            Some(cidr("fd00::/125")),
        );
        let (v4, v6) = svc.allocate_next(None, "pod-a", "default").unwrap();
        assert!(v4.is_some());
        assert!(v6.is_some());
    }

    #[tokio::test]
    async fn allocate_next_family_ipv4_only() {
        let svc = DefaultIpamService::new(
            gateway_ip(),
            Some(cidr("10.0.0.0/29")),
            Some(cidr("fd00::/125")),
        );
        let (v4, v6) = svc
            .allocate_next(Some(Family::Ipv4), "pod-a", "default")
            .unwrap();
        assert!(v4.is_some());
        assert!(v6.is_none());
    }

    #[tokio::test]
    async fn allocate_next_family_ipv6_only() {
        let svc = DefaultIpamService::new(
            gateway_ip(),
            Some(cidr("10.0.0.0/29")),
            Some(cidr("fd00::/125")),
        );
        let (v4, v6) = svc
            .allocate_next(Some(Family::Ipv6), "pod-a", "default")
            .unwrap();
        assert!(v4.is_none());
        assert!(v6.is_some());
    }

    #[tokio::test]
    async fn allocate_next_silently_skips_explicitly_requested_but_unconfigured_family() {
        let svc = DefaultIpamService::new(gateway_ip(), None, Some(cidr("fd00::/125")));
        assert_eq!(
            svc.allocate_next(Some(Family::Ipv4), "pod-a", "default"),
            Ok((None, None))
        );
    }

    #[tokio::test]
    async fn allocate_next_silently_skips_unconfigured_family_when_none_requested() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let (v4, v6) = svc.allocate_next(None, "pod-a", "default").unwrap();
        assert!(v4.is_some());
        assert!(v6.is_none());
    }

    #[tokio::test]
    async fn allocate_next_rolls_back_ipv6_when_ipv4_allocation_fails() {
        let svc = DefaultIpamService::new(
            gateway_ip(),
            Some(cidr("10.0.0.1/32")),
            Some(cidr("fd00::/125")),
        );
        svc.allocate_ip("10.0.0.1".parse().unwrap(), "pod-pre", "default")
            .unwrap();

        let result = svc.allocate_next(None, "pod-a", "default");
        assert_eq!(result, Err(IpamError::RangeFull));

        let (allocv4, allocv6, _status) = svc.dump().await.unwrap();
        assert_eq!(allocv4.len(), 1); // only the pre-allocated address
        assert!(allocv6.is_empty()); // the transient ipv6 allocation was rolled back
    }

    #[tokio::test]
    async fn allocate_next_skips_excluded_ip_and_consumes_it() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/30")), None); // usable: .1, .2
        svc.exclude_ip("10.0.0.1".parse().unwrap(), "reserved", "default");
        let (v4, _) = svc
            .allocate_next(Some(Family::Ipv4), "pod-a", "default")
            .unwrap();
        assert_eq!(v4.unwrap().ip, "10.0.0.2".parse::<IpAddr>().unwrap());
        assert_eq!(
            svc.allocate_next(Some(Family::Ipv4), "pod-b", "default"),
            Err(IpamError::RangeFull)
        );
    }

    #[tokio::test]
    async fn release_ip_requires_non_empty_pool() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert_eq!(
            svc.release_ip(ip, ""),
            Err(IpamError::PoolRequiredForRelease(ip.to_string()))
        );
    }

    #[tokio::test]
    async fn release_ip_frees_capacity_and_clears_owner() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        svc.allocate_ip(ip, "pod-a", "default").unwrap();
        svc.release_ip(ip, "default").unwrap();
        let (allocv4, _, _) = svc.dump().await.unwrap();
        assert!(allocv4.is_empty());
        svc.allocate_ip(ip, "pod-b", "default").unwrap();
    }

    #[tokio::test]
    async fn release_ip_disabled_family() {
        let svc = DefaultIpamService::new(gateway_ip(), None, None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert_eq!(svc.release_ip(ip, "default"), Err(IpamError::Ipv4Disabled));

        let ip6: IpAddr = "fd00::1".parse().unwrap();
        assert_eq!(svc.release_ip(ip6, "default"), Err(IpamError::Ipv6Disabled));
    }

    #[tokio::test]
    async fn dump_reports_dead_branch_status_when_nothing_configured() {
        let svc = DefaultIpamService::new(gateway_ip(), None, None);
        let (allocv4, allocv6, status) = svc.dump().await.unwrap();
        assert!(allocv4.is_empty());
        assert!(allocv6.is_empty());
        assert_eq!(status, ", ");
    }

    #[tokio::test]
    async fn dump_reports_allocated_ip_with_owner_under_default_pool() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        svc.allocate_ip(ip, "pod-a", "default").unwrap();

        let (allocv4, allocv6, status) = svc.dump().await.unwrap();
        assert_eq!(allocv4.get("10.0.0.1"), Some(&"pod-a".to_string()));
        assert!(allocv6.is_empty());
        assert_eq!(status, "IPv4: 1/6 allocated from 10.0.0.0/29, ");
    }

    #[tokio::test]
    async fn dump_owner_lookup_misses_for_non_default_pool_with_hostscope_allocator() {
        let svc = DefaultIpamService::new(gateway_ip(), Some(cidr("10.0.0.0/29")), None);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        svc.allocate_ip(ip, "pod-a", "custom-pool").unwrap();

        let (allocv4, _, _) = svc.dump().await.unwrap();
        assert_eq!(allocv4.get("10.0.0.1"), Some(&String::new()));
    }
}
