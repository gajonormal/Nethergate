//! Local observations only. No probes, publication, or permission changes.
use crate::connection::Adapter;
use serde_json::{json, Value};
use std::net::IpAddr;

#[derive(Default)]
pub struct Monitor {
    baseline: Option<Vec<String>>,
    candidate: Option<Vec<String>>,
    revision: u64,
    changed_at: Option<u128>,
    error: Option<String>,
}
impl Monitor {
    /// Confirm two consecutive observations to avoid reacting to a transient adapter list.
    pub fn observe(&mut self, observation: Result<Vec<Adapter>, String>, now: u128) -> bool {
        let adapters = match observation {
            Ok(adapters) => adapters,
            Err(error) => {
                self.error = Some(error);
                self.candidate = None;
                return false;
            }
        };
        self.error = None;
        let mut keys: Vec<_> = adapters
            .iter()
            .map(|a| format!("{}|{}|{:?}", a.name, a.ip, a.gateway))
            .collect();
        keys.sort();
        keys.dedup();
        if self.baseline.is_none() {
            self.baseline = Some(keys);
            return false;
        }
        if self.baseline.as_ref() == Some(&keys) {
            self.candidate = None;
            return false;
        }
        if self.candidate.as_ref() != Some(&keys) {
            self.candidate = Some(keys);
            return false;
        }
        self.baseline = Some(keys);
        self.candidate = None;
        self.revision += 1;
        self.changed_at = Some(now);
        true
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn view(&self, ip: IpAddr, adapters: &[Adapter], discovery_error: Option<&str>) -> Value {
        let bind_state = if ip.is_loopback() || ip.is_unspecified() {
            "local_or_all"
        } else if discovery_error.is_some() {
            "unknown"
        } else if adapters.iter().any(|a| IpAddr::V4(a.ip) == ip) {
            "present"
        } else {
            "missing"
        };
        json!({"supported":cfg!(windows),"revision":self.revision,"changedAt":self.changed_at,
            "hasLocalNetwork":if discovery_error.is_none() {Some(!adapters.is_empty())} else {None},
            "checking":self.candidate.is_some(),"error":discovery_error.or(self.error.as_deref()),
            "bindState":bind_state,"pollSeconds":3})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn adapter(ip: &str, gateway: &str) -> Adapter {
        Adapter {
            name: "Wi-Fi".into(),
            ip: ip.parse().unwrap(),
            gateway: Some(gateway.parse().unwrap()),
        }
    }
    #[test]
    fn confirms_changes_ignores_order_and_transients_and_tracks_reconnection() {
        let mut monitor = Monitor::default();
        let a = adapter("192.168.1.5", "192.168.1.1");
        let b = adapter("192.168.2.5", "192.168.2.1");
        assert!(!monitor.observe(Ok(vec![a.clone(), b.clone()]), 1));
        assert!(!monitor.observe(Ok(vec![b.clone(), a.clone()]), 2));
        assert!(!monitor.observe(Ok(vec![]), 3));
        assert!(!monitor.observe(Ok(vec![a.clone(), b]), 4));
        assert!(!monitor.observe(Ok(vec![a.clone()]), 5));
        assert!(monitor.observe(Ok(vec![a.clone()]), 6));
        assert!(!monitor.observe(Err("failed".into()), 7));
        assert_eq!(monitor.revision(), 1);
        assert!(!monitor.observe(Ok(vec![]), 8));
        assert!(monitor.observe(Ok(vec![]), 9));
        assert!(!monitor.observe(Ok(vec![a.clone()]), 10));
        assert!(monitor.observe(Ok(vec![a]), 11));
        assert_eq!(monitor.revision(), 3);
    }
    #[test]
    fn gateway_changes_count_and_errors_do_not_claim_an_ip_is_missing() {
        let mut monitor = Monitor::default();
        let a = adapter("192.168.1.5", "192.168.1.1");
        monitor.observe(Ok(vec![a.clone()]), 1);
        let b = adapter("192.168.1.5", "192.168.1.254");
        assert!(!monitor.observe(Ok(vec![b.clone()]), 2));
        assert!(monitor.observe(Ok(vec![b.clone()]), 3));
        assert_eq!(
            monitor.view(a.ip.into(), &[b], None)["bindState"],
            "present"
        );
        assert_eq!(
            monitor.view(a.ip.into(), &[], Some("error"))["bindState"],
            "unknown"
        );
        assert_eq!(monitor.view(a.ip.into(), &[], None)["bindState"], "missing");
    }
}
