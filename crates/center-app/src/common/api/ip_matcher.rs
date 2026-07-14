//! Lightweight, platform-neutral IP/CIDR matcher for Center listener policy.

use std::net::IpAddr;

use ipnet::IpNet;

pub fn validate_ip_or_cidr(value: &str) -> Result<(), String> {
    to_cidr_format(value)?
        .parse::<IpNet>()
        .map(|_| ())
        .map_err(|error| format!("Invalid IP or CIDR '{value}': {error}"))
}

pub fn to_cidr_format(value: &str) -> Result<String, String> {
    if value.contains('/') {
        return Ok(value.to_string());
    }
    let ip: IpAddr = value
        .parse()
        .map_err(|_| format!("Invalid IP address: {value}"))?;
    Ok(format!("{value}/{}", if ip.is_ipv4() { 32 } else { 128 }))
}

/// Flat CIDR matcher retaining the small API used by both listener layers.
pub struct IpRadixMatcher {
    entries: Vec<(IpNet, String)>,
}

impl IpRadixMatcher {
    pub fn builder() -> IpRadixMatcherBuilder {
        IpRadixMatcherBuilder {
            entries: Vec::new(),
        }
    }

    pub fn matched_group(&self, ip: &IpAddr) -> Option<&str> {
        self.entries
            .iter()
            .find_map(|(network, group)| network.contains(ip).then_some(group.as_str()))
    }
}

pub struct IpRadixMatcherBuilder {
    entries: Vec<(IpNet, String)>,
}

impl IpRadixMatcherBuilder {
    pub fn insert(&mut self, cidr: &str, group: &str) -> Result<(), String> {
        let network = cidr
            .parse::<IpNet>()
            .map_err(|error| format!("Invalid CIDR '{cidr}': {error}"))?;
        self.entries.push((network, group.to_string()));
        Ok(())
    }

    pub fn build(self) -> Result<IpRadixMatcher, String> {
        Ok(IpRadixMatcher {
            entries: self.entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ipv4_ipv6_and_bare_hosts() {
        let mut builder = IpRadixMatcher::builder();
        builder.insert("10.0.0.0/8", "v4").unwrap();
        builder.insert("2001:db8::/32", "v6").unwrap();
        builder
            .insert(&to_cidr_format("192.0.2.1").unwrap(), "host")
            .unwrap();
        let matcher = builder.build().unwrap();
        assert_eq!(
            matcher.matched_group(&"10.1.2.3".parse().unwrap()),
            Some("v4")
        );
        assert_eq!(
            matcher.matched_group(&"2001:db8::1".parse().unwrap()),
            Some("v6")
        );
        assert_eq!(
            matcher.matched_group(&"192.0.2.1".parse().unwrap()),
            Some("host")
        );
        assert_eq!(matcher.matched_group(&"203.0.113.1".parse().unwrap()), None);
    }
}
