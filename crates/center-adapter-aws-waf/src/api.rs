use async_trait::async_trait;
use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};

use crate::{
    AwsWafAssociation, AwsWafAssociationTarget, AwsWafCreateWebAclRequest, AwsWafIpSet,
    AwsWafIpSetId, AwsWafIpSetPage, AwsWafIpSetRevision, AwsWafManagedRuleGroupCatalogEntry,
    AwsWafRule, AwsWafScope, AwsWafWebAcl, AwsWafWebAclId, AwsWafWebAclPage, AwsWafWebAclRevision,
};

pub type AwsWafApiResult<T> = Result<T, NormalizedProviderError>;

/// Credential-owning AWS WAFv2 transport seam.
///
/// A production transport must verify its AWS account with STS before this
/// trait is exposed to the adapter. The typed scope is passed to every call so
/// a global CloudFront ACL can never be accidentally sent to a regional WAF
/// endpoint.
#[async_trait]
pub trait AwsWafApi: Send + Sync {
    fn verified_account_id(&self) -> &str;
    fn credential_revision(&self) -> &str;

    async fn list_web_acls(
        &self,
        scope: &AwsWafScope,
        next_marker: Option<&str>,
        limit: u16,
    ) -> AwsWafApiResult<AwsWafWebAclPage>;

    async fn get_web_acl(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Option<AwsWafWebAcl>>;

    async fn list_ip_sets(
        &self,
        _: &AwsWafScope,
        _: Option<&str>,
        _: u16,
    ) -> AwsWafApiResult<AwsWafIpSetPage> {
        Err(ip_set_unavailable())
    }
    async fn get_ip_set(
        &self,
        _: &AwsWafScope,
        _: &AwsWafIpSetId,
    ) -> AwsWafApiResult<Option<AwsWafIpSet>> {
        Err(ip_set_unavailable())
    }
    async fn create_ip_set(&self, _: &AwsWafIpSet) -> AwsWafApiResult<AwsWafIpSet> {
        Err(ip_set_unavailable())
    }
    async fn update_ip_set(
        &self,
        _: &AwsWafIpSetRevision,
        _: &AwsWafIpSet,
    ) -> AwsWafApiResult<AwsWafIpSet> {
        Err(ip_set_unavailable())
    }
    async fn delete_ip_set(&self, _: &AwsWafIpSetRevision, _: &str) -> AwsWafApiResult<()> {
        Err(ip_set_unavailable())
    }
    async fn list_managed_rule_groups(
        &self,
        _: &AwsWafScope,
    ) -> AwsWafApiResult<Vec<AwsWafManagedRuleGroupCatalogEntry>> {
        Err(ip_set_unavailable())
    }
    async fn check_capacity(&self, _: &AwsWafScope, _: &[AwsWafRule]) -> AwsWafApiResult<u32> {
        Err(ip_set_unavailable())
    }

    async fn create_web_acl(
        &self,
        request: &AwsWafCreateWebAclRequest,
    ) -> AwsWafApiResult<AwsWafWebAcl>;

    /// Updates exactly one complete Web ACL using the supplied WAF lock token.
    /// Implementations must make at most one provider request.
    async fn update_web_acl(
        &self,
        revision: &AwsWafWebAclRevision,
        desired: &AwsWafWebAcl,
    ) -> AwsWafApiResult<AwsWafWebAcl>;

    /// Deletes exactly one complete Web ACL using the supplied WAF lock token.
    /// Implementations must make at most one provider request.
    async fn delete_web_acl(&self, revision: &AwsWafWebAclRevision) -> AwsWafApiResult<()>;

    async fn list_associations(
        &self,
        scope: &AwsWafScope,
        web_acl: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Vec<AwsWafAssociation>>;

    /// The regional association seam is intentionally separate from
    /// CloudFront. CLD-29A owns CloudFront `WebACLId` association.
    async fn associate_regional_resource(
        &self,
        scope: &AwsWafScope,
        target: &AwsWafAssociationTarget,
        web_acl: &AwsWafWebAclId,
    ) -> AwsWafApiResult<()>;

    async fn disassociate_regional_resource(
        &self,
        scope: &AwsWafScope,
        target: &AwsWafAssociationTarget,
    ) -> AwsWafApiResult<()>;
}

fn ip_set_unavailable() -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        "aws_waf_ip_set_transport_unavailable",
        "Sanitized AWS WAF adapter error",
        None,
        None,
    )
    .expect("fixed error")
}
