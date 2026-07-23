//! Server-only ownership proof encoded in AWS WAF Rule.Name.
//!
//! RuleLabels cannot be used by every WAF rule family and increase WCU. A
//! bounded reference and HMAC tag in Rule.Name work for managed, IP-set, and
//! rate rules. A prefix never establishes ownership; full verification binds
//! the provider account, AWS account, scope, and immutable Web ACL name.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use edgion_center_adapter_aws_waf::AwsWafScope;

const PREFIX: &str = "ec_";
const TAG_BYTES: usize = 16;
const MAX_REFERENCE_BYTES: usize = 92;
type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub(crate) struct OwnershipKey([u8; 32]);

impl OwnershipKey {
    pub(crate) fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    pub(crate) fn rule_name(
        &self,
        provider: &str,
        aws: &str,
        scope: &AwsWafScope,
        acl: &str,
        reference: &str,
    ) -> Option<String> {
        valid_reference(reference).then(|| {
            format!(
                "{PREFIX}{reference}_{}",
                self.tag(provider, aws, scope, acl, reference)
            )
        })
    }

    pub(crate) fn verify(
        &self,
        provider: &str,
        aws: &str,
        scope: &AwsWafScope,
        acl: &str,
        name: &str,
    ) -> Option<String> {
        let (reference, tag) = name.strip_prefix(PREFIX)?.rsplit_once('_')?;
        if !valid_reference(reference) || tag.len() != TAG_BYTES * 2 {
            return None;
        }
        let expected = self.tag(provider, aws, scope, acl, reference);
        constant_time_eq(&hex_decode(tag)?, &hex_decode(&expected)?).then(|| reference.to_string())
    }

    fn tag(
        &self,
        provider: &str,
        aws: &str,
        scope: &AwsWafScope,
        acl: &str,
        reference: &str,
    ) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("fixed HMAC key");
        let scope = scope_key(scope);
        for value in [
            b"edgion-center/aws-waf-rule-owner/v1".as_slice(),
            provider.as_bytes(),
            aws.as_bytes(),
            scope.as_bytes(),
            acl.as_bytes(),
            reference.as_bytes(),
        ] {
            mac.update(&(value.len() as u32).to_be_bytes());
            mac.update(value);
        }
        hex_encode(&mac.finalize().into_bytes()[..TAG_BYTES])
    }
}

fn valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REFERENCE_BYTES
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn scope_key(scope: &AwsWafScope) -> String {
    match scope {
        AwsWafScope::Cloudfront => "cloudfront".to_string(),
        AwsWafScope::Regional { region } => format!("regional:{region}"),
    }
}
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|chunk| u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok())
        .collect()
}
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> AwsWafScope {
        AwsWafScope::Regional {
            region: "us-west-2".to_string(),
        }
    }
    fn name(key: &OwnershipKey) -> String {
        key.rule_name(
            "provider-a",
            "123456789012",
            &scope(),
            "orders",
            "block-bots",
        )
        .unwrap()
    }
    #[test]
    fn signed_names_round_trip_for_all_supported_rule_families() {
        let key = OwnershipKey::new([7; 32]);
        assert_eq!(
            key.verify(
                "provider-a",
                "123456789012",
                &scope(),
                "orders",
                &name(&key)
            ),
            Some("block-bots".to_string())
        );
    }
    #[test]
    fn forgery_duplicate_and_transplant_fail_closed() {
        let key = OwnershipKey::new([7; 32]);
        let name = name(&key);
        assert_eq!(
            key.verify(
                "provider-a",
                "123456789012",
                &scope(),
                "orders",
                &format!("{name}0")
            ),
            None
        );
        assert_eq!(
            key.verify("provider-a", "123456789012", &scope(), "payments", &name),
            None
        );
        assert_eq!(
            key.verify(
                "provider-a",
                "123456789012",
                &AwsWafScope::Cloudfront,
                "orders",
                &name
            ),
            None
        );
    }
}
