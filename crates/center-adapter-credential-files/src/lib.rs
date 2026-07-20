//! Deployment-owned, capability-confined mounted credential aliases.
//!
//! This SDK-free adapter resolves an operator-configured alias to bounded bytes
//! beneath one pre-opened directory. It does not construct provider clients,
//! inspect credentials, call cloud APIs, or read Kubernetes Secrets.

use std::{
    collections::BTreeSet,
    fmt,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[cfg(unix)]
use cap_std::fs::OpenOptionsExt;
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use edgion_center_core::{CloudProvider, CloudResourceId, CredentialRef};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

const MAX_IDENTITY_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 4096;
const MAX_BINDINGS: usize = 1024;
const MAX_CREDENTIAL_BYTES: usize = 16 * 1024;
const REVISION_KEY_BYTES: usize = 32;
const REVISION_DOMAIN: &[u8] = b"edgion-center/mounted-credential-revision/v1";

/// Closed material-purpose binding. Adding another format requires an explicit
/// provider consumer contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPurpose {
    CloudflareApiToken,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedCredentialBinding {
    pub credential_ref: String,
    pub provider_account_id: String,
    pub provider: CloudProvider,
    pub purpose: CredentialPurpose,
    /// Strict path relative to `root_directory`.
    pub file: String,
}

impl fmt::Debug for MountedCredentialBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MountedCredentialBinding")
            .field("provider", &self.provider)
            .field("purpose", &self.purpose)
            .field("credential_ref", &"[REDACTED]")
            .field("provider_account_id", &"[REDACTED]")
            .field("file", &"[REDACTED]")
            .finish()
    }
}

/// Strict, default-off configuration shared by both deployment modes.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MountedCredentialConfig {
    pub enabled: bool,
    /// Absolute directory opened once as the resolver's filesystem capability.
    pub root_directory: Option<String>,
    /// Strict path relative to `root_directory` containing exactly 32 non-zero bytes.
    pub revision_key_file: Option<String>,
    pub bindings: Vec<MountedCredentialBinding>,
}

impl fmt::Debug for MountedCredentialConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MountedCredentialConfig")
            .field("enabled", &self.enabled)
            .field(
                "root_directory",
                &self.root_directory.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "revision_key_file",
                &self.revision_key_file.as_ref().map(|_| "[REDACTED]"),
            )
            .field("binding_count", &self.bindings.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialConfigError {
    MissingRootDirectory,
    InvalidRootDirectory,
    MissingRevisionKey,
    InvalidRevisionKey,
    NoBindings,
    TooManyBindings,
    InvalidBinding,
    DuplicateBinding,
}

impl fmt::Display for CredentialConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingRootDirectory => "mounted credential root is not configured",
            Self::InvalidRootDirectory => "mounted credential root is invalid",
            Self::MissingRevisionKey => "mounted credential revision key is not configured",
            Self::InvalidRevisionKey => "mounted credential revision key is invalid",
            Self::NoBindings => "mounted credential bindings are empty",
            Self::TooManyBindings => "mounted credential binding limit was exceeded",
            Self::InvalidBinding => "mounted credential binding is invalid",
            Self::DuplicateBinding => "mounted credential binding is duplicated",
        })
    }
}

impl std::error::Error for CredentialConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialResolutionError {
    ReferenceNotFound,
    ScopeMismatch,
    Unreadable,
    NotRegular,
    TooLarge,
    Empty,
    UnsafePermissions,
}

impl fmt::Display for CredentialResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ReferenceNotFound => "mounted credential reference was not found",
            Self::ScopeMismatch => "mounted credential reference scope did not match",
            Self::Unreadable => "mounted credential file was unavailable",
            Self::NotRegular => "mounted credential path was not a regular file",
            Self::TooLarge => "mounted credential file exceeded its size limit",
            Self::Empty => "mounted credential file was empty",
            Self::UnsafePermissions => "mounted credential file permissions were unsafe",
        })
    }
}

impl std::error::Error for CredentialResolutionError {}

pub struct ResolveCredentialRequest<'a> {
    pub provider_account_id: &'a CloudResourceId,
    pub provider: &'a CloudProvider,
    pub purpose: CredentialPurpose,
    pub credential_ref: &'a CredentialRef,
}

pub struct ResolvedCredentialRevision(String);

impl ResolvedCredentialRevision {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ResolvedCredentialRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResolvedCredentialRevision([REDACTED])")
    }
}

pub struct ResolvedCredential {
    material: Zeroizing<Vec<u8>>,
    revision: ResolvedCredentialRevision,
}

impl ResolvedCredential {
    pub fn with_bytes<T>(&self, consume: impl FnOnce(&[u8]) -> T) -> T {
        consume(self.material.as_slice())
    }

    pub fn revision(&self) -> &ResolvedCredentialRevision {
        &self.revision
    }
}

impl fmt::Debug for ResolvedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedCredential")
            .field("material", &"[REDACTED]")
            .field("revision", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
struct ValidatedBinding {
    credential_ref: CredentialRef,
    provider_account_id: CloudResourceId,
    provider: CloudProvider,
    purpose: CredentialPurpose,
    file: PathBuf,
}

/// A resolver confined to the directory capability opened during construction.
pub struct MountedCredentialResolver {
    root: Arc<Dir>,
    revision_key: Arc<Zeroizing<[u8; REVISION_KEY_BYTES]>>,
    bindings: Vec<ValidatedBinding>,
}

impl fmt::Debug for MountedCredentialResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MountedCredentialResolver")
            .field("root", &"[REDACTED]")
            .field("revision_key", &"[REDACTED]")
            .field("binding_count", &self.bindings.len())
            .finish()
    }
}

impl MountedCredentialResolver {
    /// Builds an optional resolver. Disabled configuration performs no filesystem access.
    pub async fn from_config(
        config: &MountedCredentialConfig,
    ) -> Result<Option<Self>, CredentialConfigError> {
        if !config.enabled {
            return Ok(None);
        }

        let root = validate_absolute_root(
            config
                .root_directory
                .as_deref()
                .ok_or(CredentialConfigError::MissingRootDirectory)?,
        )?;
        let revision_key_file = validate_locator(
            config
                .revision_key_file
                .as_deref()
                .ok_or(CredentialConfigError::MissingRevisionKey)?,
        )
        .map_err(|_| CredentialConfigError::InvalidRevisionKey)?;
        if config.bindings.is_empty() {
            return Err(CredentialConfigError::NoBindings);
        }
        if config.bindings.len() > MAX_BINDINGS {
            return Err(CredentialConfigError::TooManyBindings);
        }

        let mut bindings = Vec::with_capacity(config.bindings.len());
        let mut identities = BTreeSet::new();
        for binding in &config.bindings {
            if binding.credential_ref.len() > MAX_IDENTITY_BYTES
                || binding.provider_account_id.len() > MAX_IDENTITY_BYTES
                || binding.provider != CloudProvider::Cloudflare
                || binding.purpose != CredentialPurpose::CloudflareApiToken
            {
                return Err(CredentialConfigError::InvalidBinding);
            }
            let credential_ref = CredentialRef::new(binding.credential_ref.clone())
                .map_err(|_| CredentialConfigError::InvalidBinding)?;
            let provider_account_id = CloudResourceId::new(binding.provider_account_id.clone())
                .map_err(|_| CredentialConfigError::InvalidBinding)?;
            let file = validate_locator(&binding.file)
                .map_err(|_| CredentialConfigError::InvalidBinding)?;
            let identity = (
                credential_ref.as_str().to_string(),
                provider_account_id.as_str().to_string(),
                provider_tag(&binding.provider),
                binding.purpose,
            );
            if !identities.insert(identity) {
                return Err(CredentialConfigError::DuplicateBinding);
            }
            bindings.push(ValidatedBinding {
                credential_ref,
                provider_account_id,
                provider: binding.provider.clone(),
                purpose: binding.purpose,
                file,
            });
        }

        let (root, key) = tokio::task::spawn_blocking(move || {
            let root = Dir::open_ambient_dir(root, ambient_authority())
                .map_err(|_| CredentialConfigError::InvalidRootDirectory)?;
            #[cfg(unix)]
            {
                use cap_std::fs::MetadataExt as _;
                let metadata = root
                    .dir_metadata()
                    .map_err(|_| CredentialConfigError::InvalidRootDirectory)?;
                if metadata.mode() & 0o022 != 0 {
                    return Err(CredentialConfigError::InvalidRootDirectory);
                }
            }
            let key = read_regular_file(&root, &revision_key_file, REVISION_KEY_BYTES)
                .map_err(|_| CredentialConfigError::InvalidRevisionKey)?;
            if key.len() != REVISION_KEY_BYTES || key.iter().all(|byte| *byte == 0) {
                return Err(CredentialConfigError::InvalidRevisionKey);
            }
            let mut revision_key = Zeroizing::new([0_u8; REVISION_KEY_BYTES]);
            revision_key.copy_from_slice(&key);
            Ok((root, revision_key))
        })
        .await
        .map_err(|_| CredentialConfigError::InvalidRootDirectory)??;

        Ok(Some(Self {
            root: Arc::new(root),
            revision_key: Arc::new(key),
            bindings,
        }))
    }

    pub async fn resolve(
        &self,
        request: ResolveCredentialRequest<'_>,
    ) -> Result<ResolvedCredential, CredentialResolutionError> {
        let matching_ref: Vec<_> = self
            .bindings
            .iter()
            .filter(|binding| binding.credential_ref == *request.credential_ref)
            .collect();
        if matching_ref.is_empty() {
            return Err(CredentialResolutionError::ReferenceNotFound);
        }
        let binding = matching_ref
            .into_iter()
            .find(|binding| {
                binding.provider_account_id == *request.provider_account_id
                    && binding.provider == *request.provider
                    && binding.purpose == request.purpose
            })
            .ok_or(CredentialResolutionError::ScopeMismatch)?;

        // Match and copy only non-secret authority before any filesystem I/O.
        let file = binding.file.clone();
        let root = self.root.clone();
        let revision_key = self.revision_key.clone();
        let account_id = request.provider_account_id.clone();
        let provider = request.provider.clone();
        let purpose = request.purpose;
        let credential_ref = request.credential_ref.clone();
        tokio::task::spawn_blocking(move || {
            let material = read_regular_file(&root, &file, MAX_CREDENTIAL_BYTES)?;
            if material.is_empty() {
                return Err(CredentialResolutionError::Empty);
            }
            let revision = revision(
                revision_key.as_slice(),
                &account_id,
                &provider,
                purpose,
                &credential_ref,
                &material,
            );
            Ok(ResolvedCredential {
                material,
                revision: ResolvedCredentialRevision(revision),
            })
        })
        .await
        .map_err(|_| CredentialResolutionError::Unreadable)?
    }
}

fn validate_absolute_root(value: &str) -> Result<PathBuf, CredentialConfigError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.chars().any(char::is_control)
        || !path.is_absolute()
    {
        return Err(CredentialConfigError::InvalidRootDirectory);
    }
    Ok(path.to_path_buf())
}

fn validate_locator(value: &str) -> Result<PathBuf, ()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.chars().any(char::is_control)
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(());
    }
    Ok(path.to_path_buf())
}

fn read_regular_file(
    root: &Dir,
    path: &Path,
    max_bytes: usize,
) -> Result<Zeroizing<Vec<u8>>, CredentialResolutionError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options
        .custom_flags((rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC).bits() as i32);
    let mut file = root
        .open_with(path, &options)
        .map_err(|_| CredentialResolutionError::Unreadable)?;
    let metadata = file
        .metadata()
        .map_err(|_| CredentialResolutionError::Unreadable)?;
    if !metadata.is_file() {
        return Err(CredentialResolutionError::NotRegular);
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt as _;
        if metadata.mode() & 0o022 != 0 {
            return Err(CredentialResolutionError::UnsafePermissions);
        }
    }
    if metadata.len() > max_bytes as u64 {
        return Err(CredentialResolutionError::TooLarge);
    }
    let mut material = Zeroizing::new(Vec::with_capacity(metadata.len() as usize));
    file.by_ref()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut material)
        .map_err(|_| CredentialResolutionError::Unreadable)?;
    if material.len() > max_bytes {
        return Err(CredentialResolutionError::TooLarge);
    }
    let final_len = file
        .metadata()
        .map_err(|_| CredentialResolutionError::Unreadable)?
        .len();
    if final_len != metadata.len() || final_len != material.len() as u64 {
        return Err(CredentialResolutionError::Unreadable);
    }
    Ok(material)
}

fn revision(
    key: &[u8],
    account_id: &CloudResourceId,
    provider: &CloudProvider,
    purpose: CredentialPurpose,
    credential_ref: &CredentialRef,
    material: &[u8],
) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .expect("HMAC accepts the validated revision key length");
    for part in [
        REVISION_DOMAIN,
        account_id.as_str().as_bytes(),
        provider_tag(provider).as_bytes(),
        purpose_tag(purpose).as_bytes(),
        credential_ref.as_str().as_bytes(),
        material,
    ] {
        mac.update(&(part.len() as u64).to_be_bytes());
        mac.update(part);
    }
    let bytes = mac.finalize().into_bytes();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("hmac-sha256-v1:{encoded}")
}

fn provider_tag(provider: &CloudProvider) -> &'static str {
    match provider {
        CloudProvider::Cloudflare => "cloudflare",
        CloudProvider::Aws => "aws",
        CloudProvider::GoogleCloud => "google_cloud",
    }
}

fn purpose_tag(purpose: CredentialPurpose) -> &'static str {
    match purpose {
        CredentialPurpose::CloudflareApiToken => "cloudflare_api_token",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(root: &Path) -> MountedCredentialConfig {
        MountedCredentialConfig {
            enabled: true,
            root_directory: Some(root.to_string_lossy().into_owned()),
            revision_key_file: Some("revision.key".into()),
            bindings: vec![MountedCredentialBinding {
                credential_ref: "cloudflare/main".into(),
                provider_account_id: "cf-main".into(),
                provider: CloudProvider::Cloudflare,
                purpose: CredentialPurpose::CloudflareApiToken,
                file: "token".into(),
            }],
        }
    }

    async fn resolver(root: &Path, token: &[u8]) -> MountedCredentialResolver {
        tokio::fs::write(root.join("revision.key"), [7_u8; 32])
            .await
            .unwrap();
        tokio::fs::write(root.join("token"), token).await.unwrap();
        MountedCredentialResolver::from_config(&config(root))
            .await
            .unwrap()
            .unwrap()
    }

    fn request<'a>(
        account: &'a CloudResourceId,
        provider: &'a CloudProvider,
        credential_ref: &'a CredentialRef,
    ) -> ResolveCredentialRequest<'a> {
        ResolveCredentialRequest {
            provider_account_id: account,
            provider,
            purpose: CredentialPurpose::CloudflareApiToken,
            credential_ref,
        }
    }

    #[tokio::test]
    async fn disabled_configuration_performs_no_file_access() {
        let value = MountedCredentialConfig {
            enabled: false,
            root_directory: Some("/definitely/missing".into()),
            revision_key_file: Some("missing.key".into()),
            bindings: Vec::new(),
        };
        assert!(MountedCredentialResolver::from_config(&value)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn strict_configuration_rejects_paths_keys_duplicates_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut value = config(root);
        assert!(matches!(
            MountedCredentialResolver::from_config(&value).await,
            Err(CredentialConfigError::InvalidRevisionKey)
        ));
        tokio::fs::write(root.join("revision.key"), [0_u8; 32])
            .await
            .unwrap();
        assert!(matches!(
            MountedCredentialResolver::from_config(&value).await,
            Err(CredentialConfigError::InvalidRevisionKey)
        ));
        tokio::fs::write(root.join("revision.key"), [7_u8; 32])
            .await
            .unwrap();
        value.root_directory = Some("relative-root".into());
        assert!(matches!(
            MountedCredentialResolver::from_config(&value).await,
            Err(CredentialConfigError::InvalidRootDirectory)
        ));
        value = config(root);
        value.revision_key_file = Some("/absolute/key".into());
        assert!(matches!(
            MountedCredentialResolver::from_config(&value).await,
            Err(CredentialConfigError::InvalidRevisionKey)
        ));
        value = config(root);
        value.bindings[0].file = "../token".into();
        assert!(matches!(
            MountedCredentialResolver::from_config(&value).await,
            Err(CredentialConfigError::InvalidBinding)
        ));
        value = config(root);
        value.bindings.push(value.bindings[0].clone());
        assert!(matches!(
            MountedCredentialResolver::from_config(&value).await,
            Err(CredentialConfigError::DuplicateBinding)
        ));
        value = config(root);
        value.bindings = vec![value.bindings[0].clone(); MAX_BINDINGS + 1];
        assert!(matches!(
            MountedCredentialResolver::from_config(&value).await,
            Err(CredentialConfigError::TooManyBindings)
        ));
        assert!(
            serde_yaml::from_str::<MountedCredentialConfig>("enabled: false\nunknown: true\n")
                .is_err()
        );
        assert!(serde_yaml::from_str::<MountedCredentialConfig>(
            "enabled: true\nroot_directory: /root\nrevision_key_file: key\nbindings:\n  - credential_ref: x\n    provider_account_id: y\n    provider: cloudflare\n    purpose: cloudflare_api_token\n    file: token\n    unknown: true\n"
        )
        .is_err());
    }

    #[tokio::test]
    async fn mismatch_is_rejected_before_binding_file_io_and_debug_is_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let value = resolver(dir.path(), b"secret-marker").await;
        tokio::fs::remove_file(dir.path().join("token"))
            .await
            .unwrap();
        let other_account = CloudResourceId::new("cf-other").unwrap();
        let credential_ref = CredentialRef::new("cloudflare/main").unwrap();
        let other_ref = CredentialRef::new("cloudflare/other").unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &other_account,
                    &CloudProvider::Cloudflare,
                    &credential_ref,
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::ScopeMismatch
        );
        assert_eq!(
            value
                .resolve(request(
                    &other_account,
                    &CloudProvider::Aws,
                    &credential_ref
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::ScopeMismatch
        );
        assert_eq!(
            value
                .resolve(request(
                    &other_account,
                    &CloudProvider::Cloudflare,
                    &other_ref,
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::ReferenceNotFound
        );
        let diagnostics = format!("{value:?} {:?}", config(dir.path()));
        assert!(!diagnostics.contains("secret-marker"));
        assert!(!diagnostics.contains(dir.path().to_string_lossy().as_ref()));
        assert!(!diagnostics.contains("cloudflare/main"));
        assert!(!diagnostics.contains("cf-main"));
    }

    #[tokio::test]
    async fn reads_exact_bounded_nonempty_regular_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let value = resolver(dir.path(), b"token-with-newline\n").await;
        let account = CloudResourceId::new("cf-main").unwrap();
        let credential_ref = CredentialRef::new("cloudflare/main").unwrap();
        let resolved = value
            .resolve(request(
                &account,
                &CloudProvider::Cloudflare,
                &credential_ref,
            ))
            .await
            .unwrap();
        assert_eq!(
            resolved.with_bytes(|bytes| bytes.to_vec()),
            b"token-with-newline\n"
        );
        assert!(!format!("{resolved:?}").contains("token-with-newline"));
        assert!(!format!("{:?}", resolved.revision()).contains(resolved.revision().as_str()));

        tokio::fs::write(
            dir.path().join("token"),
            vec![b'x'; MAX_CREDENTIAL_BYTES + 1],
        )
        .await
        .unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::TooLarge
        );
        tokio::fs::remove_file(dir.path().join("token"))
            .await
            .unwrap();
        tokio::fs::create_dir(dir.path().join("token"))
            .await
            .unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::NotRegular
        );
        tokio::fs::remove_dir(dir.path().join("token"))
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("token"), [])
            .await
            .unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::Empty
        );
    }

    #[tokio::test]
    async fn revisions_are_stable_and_domain_separated() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = resolver(first_dir.path(), b"token-a").await;
        let second = resolver(second_dir.path(), b"token-a").await;
        let account = CloudResourceId::new("cf-main").unwrap();
        let other_account = CloudResourceId::new("cf-other").unwrap();
        let credential_ref = CredentialRef::new("cloudflare/main").unwrap();
        let other_ref = CredentialRef::new("cloudflare/other").unwrap();
        let first_revision = first
            .resolve(request(
                &account,
                &CloudProvider::Cloudflare,
                &credential_ref,
            ))
            .await
            .unwrap()
            .revision()
            .as_str()
            .to_string();
        let second_revision = second
            .resolve(request(
                &account,
                &CloudProvider::Cloudflare,
                &credential_ref,
            ))
            .await
            .unwrap()
            .revision()
            .as_str()
            .to_string();
        assert_eq!(first_revision, second_revision);
        assert_eq!(first_revision.len(), "hmac-sha256-v1:".len() + 64);
        assert_ne!(
            first_revision,
            revision(
                &[7; 32],
                &other_account,
                &CloudProvider::Cloudflare,
                CredentialPurpose::CloudflareApiToken,
                &credential_ref,
                b"token-a"
            )
        );
        assert_ne!(
            first_revision,
            revision(
                &[7; 32],
                &account,
                &CloudProvider::Aws,
                CredentialPurpose::CloudflareApiToken,
                &credential_ref,
                b"token-a"
            )
        );
        assert_ne!(
            first_revision,
            revision(
                &[7; 32],
                &account,
                &CloudProvider::Cloudflare,
                CredentialPurpose::CloudflareApiToken,
                &other_ref,
                b"token-a"
            )
        );
        assert_ne!(
            first_revision,
            revision(
                &[8; 32],
                &account,
                &CloudProvider::Cloudflare,
                CredentialPurpose::CloudflareApiToken,
                &credential_ref,
                b"token-a"
            )
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn projected_symlink_rotation_stays_inside_root_and_escape_is_rejected() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("revision.key"), [7_u8; 32])
            .await
            .unwrap();
        tokio::fs::create_dir(dir.path().join("..2026_a"))
            .await
            .unwrap();
        tokio::fs::create_dir(dir.path().join("..2026_b"))
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("..2026_a/token"), b"a")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("..2026_b/token"), b"b")
            .await
            .unwrap();
        symlink("..2026_a", dir.path().join("..data")).unwrap();
        symlink("..data/token", dir.path().join("token")).unwrap();
        let value = MountedCredentialResolver::from_config(&config(dir.path()))
            .await
            .unwrap()
            .unwrap();
        let account = CloudResourceId::new("cf-main").unwrap();
        let credential_ref = CredentialRef::new("cloudflare/main").unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap()
                .with_bytes(|v| v.to_vec()),
            b"a"
        );
        tokio::fs::remove_file(dir.path().join("..data"))
            .await
            .unwrap();
        symlink("..2026_b", dir.path().join("..data")).unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap()
                .with_bytes(|v| v.to_vec()),
            b"b"
        );

        let outside = tempfile::tempdir().unwrap();
        tokio::fs::write(outside.path().join("secret"), b"outside")
            .await
            .unwrap();
        tokio::fs::remove_file(dir.path().join("token"))
            .await
            .unwrap();
        symlink(outside.path().join("secret"), dir.path().join("token")).unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::Unreadable
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fifo_is_opened_nonblocking_and_rejected_as_nonregular() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt as _};

        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("revision.key"), [7_u8; 32])
            .await
            .unwrap();
        let fifo = dir.path().join("token");
        let fifo = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `fifo` is a live, NUL-terminated pathname and mode contains
        // only ordinary permission bits.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let value = MountedCredentialResolver::from_config(&config(dir.path()))
            .await
            .unwrap()
            .unwrap();
        let account = CloudResourceId::new("cf-main").unwrap();
        let credential_ref = CredentialRef::new("cloudflare/main").unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::NotRegular
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn group_or_world_writable_material_is_rejected() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let value = resolver(dir.path(), b"secret").await;
        std::fs::set_permissions(
            dir.path().join("token"),
            std::fs::Permissions::from_mode(0o622),
        )
        .unwrap();
        let account = CloudResourceId::new("cf-main").unwrap();
        let credential_ref = CredentialRef::new("cloudflare/main").unwrap();
        assert_eq!(
            value
                .resolve(request(
                    &account,
                    &CloudProvider::Cloudflare,
                    &credential_ref
                ))
                .await
                .unwrap_err(),
            CredentialResolutionError::UnsafePermissions
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn group_or_world_writable_root_is_rejected() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("revision.key"), [7_u8; 32])
            .await
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(matches!(
            MountedCredentialResolver::from_config(&config(dir.path())).await,
            Err(CredentialConfigError::InvalidRootDirectory)
        ));
    }
}
