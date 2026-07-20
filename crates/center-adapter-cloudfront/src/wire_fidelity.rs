use std::{
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};

use aws_sdk_cloudfront::{
    config::{
        interceptors::{
            AfterDeserializationInterceptorContextRef, BeforeTransmitInterceptorContextRef,
        },
        ConfigBag, Intercept, RuntimeComponents,
    },
    types::DistributionConfig,
};
use xmlparser::{ElementEnd, Token, Tokenizer};
use zeroize::{Zeroize, Zeroizing};

use crate::{validation, CloudFrontApiResult};

const CLOUDFRONT_XML_NAMESPACE: &str = "http://cloudfront.amazonaws.com/doc/2020-05-31/";
const SERIALIZER_SINK_ENDPOINT: &str = "http://127.0.0.1:9";
const MAX_MUTATION_REQUEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 128;
const MAX_XML_NODES: usize = 100_000;

/// Sensitive provider bytes retained only for one mutation-planning window.
///
/// This type is deliberately neither `Debug`, `Clone`, nor serializable.
pub(crate) struct CloudFrontSensitiveWireBytes(Zeroizing<Vec<u8>>);

impl CloudFrontSensitiveWireBytes {
    pub(crate) fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

type CaptureSlot = Arc<Mutex<Option<Zeroizing<Vec<u8>>>>>;

pub(crate) struct CloudFrontResponseCapture {
    slot: CaptureSlot,
}

pub(crate) struct CloudFrontResponseCaptureHandle {
    slot: CaptureSlot,
}

impl fmt::Debug for CloudFrontResponseCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CloudFrontResponseCapture")
    }
}

pub(crate) fn cloudfront_response_capture(
) -> (CloudFrontResponseCapture, CloudFrontResponseCaptureHandle) {
    let slot = Arc::new(Mutex::new(None));
    (
        CloudFrontResponseCapture {
            slot: Arc::clone(&slot),
        },
        CloudFrontResponseCaptureHandle { slot },
    )
}

impl CloudFrontResponseCaptureHandle {
    pub(crate) fn take(self) -> CloudFrontApiResult<CloudFrontSensitiveWireBytes> {
        let bytes = self
            .slot
            .lock()
            .map_err(|_| validation("cloudfront_wire_capture_unavailable"))?
            .take()
            .ok_or_else(|| validation("cloudfront_wire_response_not_captured"))?;
        Ok(CloudFrontSensitiveWireBytes(bytes))
    }
}

impl Intercept for CloudFrontResponseCapture {
    fn name(&self) -> &'static str {
        "CloudFrontResponseCapture"
    }

    fn read_after_deserialization(
        &self,
        context: &AfterDeserializationInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if context.output_or_error().is_err() {
            return Ok(());
        }
        let Some(bytes) = context.response().body().bytes() else {
            return Ok(());
        };
        let mut slot = self
            .slot
            .lock()
            .map_err(|_| WireCaptureError("cloudfront response capture lock poisoned"))?;
        *slot = Some(Zeroizing::new(bytes.to_vec()));
        Ok(())
    }
}

struct SerializedRequestCapture {
    slot: CaptureSlot,
}

impl fmt::Debug for SerializedRequestCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SerializedRequestCapture")
    }
}

#[derive(Debug)]
struct WireCaptureError(&'static str);

#[derive(Debug)]
struct SerializationProbeAbort;

#[derive(Debug)]
struct SerializationProbeTooLarge;

impl fmt::Display for SerializationProbeAbort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cloudfront serialization probe aborted")
    }
}

impl Error for SerializationProbeAbort {}

impl fmt::Display for SerializationProbeTooLarge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cloudfront serialization probe exceeded request limit")
    }
}

impl Error for SerializationProbeTooLarge {}

impl fmt::Display for WireCaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for WireCaptureError {}

impl Intercept for SerializedRequestCapture {
    fn name(&self) -> &'static str {
        "CloudFrontSerializedRequestCapture"
    }

    fn read_after_serialization(
        &self,
        context: &BeforeTransmitInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let bytes = context
            .request()
            .body()
            .bytes()
            .ok_or(WireCaptureError("cloudfront request body was not buffered"))?;
        if bytes.len() > MAX_MUTATION_REQUEST_BYTES {
            return Err(Box::new(SerializationProbeTooLarge));
        }
        let mut slot = self
            .slot
            .lock()
            .map_err(|_| WireCaptureError("cloudfront request capture lock poisoned"))?;
        *slot = Some(Zeroizing::new(bytes.to_vec()));
        Err(Box::new(SerializationProbeAbort))
    }
}

async fn serialize_without_transmit(
    client: &aws_sdk_cloudfront::Client,
    distribution_id: &str,
    etag: &str,
    config: DistributionConfig,
) -> CloudFrontApiResult<CloudFrontSensitiveWireBytes> {
    let slot = Arc::new(Mutex::new(None));
    let result = client
        .update_distribution()
        .id(distribution_id)
        .if_match(etag)
        .distribution_config(config)
        .customize()
        .interceptor(SerializedRequestCapture {
            slot: Arc::clone(&slot),
        })
        .config_override(
            aws_sdk_cloudfront::config::Builder::new().endpoint_url(SERIALIZER_SINK_ENDPOINT),
        )
        .send()
        .await;
    let error = match result {
        Ok(_) => return Err(validation("cloudfront_wire_probe_was_transmitted")),
        Err(error) => error,
    };
    if error_chain_contains::<SerializationProbeTooLarge>(&error) {
        return Err(validation("cloudfront_wire_request_too_large"));
    }
    if !error_chain_contains::<SerializationProbeAbort>(&error) {
        return Err(validation("cloudfront_wire_probe_abort_unverified"));
    }
    let bytes = slot
        .lock()
        .map_err(|_| validation("cloudfront_wire_capture_unavailable"))?
        .take()
        .ok_or_else(|| validation("cloudfront_wire_request_not_captured"))?;
    Ok(CloudFrontSensitiveWireBytes(bytes))
}

fn error_chain_contains<T: Error + 'static>(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.downcast_ref::<T>().is_some() {
            return true;
        }
        current = error.source();
    }
    false
}

/// Private, short-lived evidence that this exact observed configuration round-trips through the
/// pinned SDK and that the desired wire changes only the root `Enabled` scalar.
pub(crate) struct CloudFrontWireFidelityEvidence {
    desired_wire: CloudFrontSensitiveWireBytes,
}

impl CloudFrontWireFidelityEvidence {
    pub(crate) fn desired_wire(&self) -> &[u8] {
        self.desired_wire.as_slice()
    }
}

pub(crate) async fn admit_enablement_wire_fidelity(
    client: &aws_sdk_cloudfront::Client,
    distribution_id: &str,
    etag: &str,
    observed_wire: &CloudFrontSensitiveWireBytes,
    current: &DistributionConfig,
    desired: &DistributionConfig,
) -> CloudFrontApiResult<CloudFrontWireFidelityEvidence> {
    let current_wire =
        serialize_without_transmit(client, distribution_id, etag, current.clone()).await?;
    compare_documents(observed_wire.as_slice(), current_wire.as_slice())?;

    let desired_wire =
        serialize_without_transmit(client, distribution_id, etag, desired.clone()).await?;
    validate_root_enabled_only(current_wire.as_slice(), desired_wire.as_slice())?;
    Ok(CloudFrontWireFidelityEvidence { desired_wire })
}

#[derive(PartialEq, Eq)]
struct CanonicalElement {
    name: String,
    text: String,
    children: Vec<CanonicalElement>,
}

impl Drop for CanonicalElement {
    fn drop(&mut self) {
        self.name.zeroize();
        self.text.zeroize();
    }
}

struct PendingElement {
    name: String,
    prefix: String,
    attributes: Vec<(String, String, String)>,
}

impl Drop for PendingElement {
    fn drop(&mut self) {
        self.name.zeroize();
        self.prefix.zeroize();
        for (prefix, local, value) in &mut self.attributes {
            prefix.zeroize();
            local.zeroize();
            value.zeroize();
        }
    }
}

fn parse_document(input: &[u8]) -> CloudFrontApiResult<CanonicalElement> {
    let text =
        std::str::from_utf8(input).map_err(|_| validation("cloudfront_wire_xml_invalid_utf8"))?;
    let mut stack: Vec<CanonicalElement> = Vec::new();
    let mut pending: Option<PendingElement> = None;
    let mut root: Option<CanonicalElement> = None;
    let mut nodes = 0usize;

    for token in Tokenizer::from(text) {
        let token = token.map_err(|_| validation("cloudfront_wire_xml_invalid"))?;
        match token {
            Token::Declaration { encoding, .. } => {
                if root.is_some()
                    || !stack.is_empty()
                    || encoding.is_some_and(|value| !value.as_str().eq_ignore_ascii_case("utf-8"))
                {
                    return Err(validation("cloudfront_wire_xml_invalid_declaration"));
                }
            }
            Token::ElementStart { prefix, local, .. } => {
                if pending.is_some() || !prefix.as_str().is_empty() {
                    return Err(validation("cloudfront_wire_xml_namespace_mismatch"));
                }
                pending = Some(PendingElement {
                    name: local.as_str().to_string(),
                    prefix: prefix.as_str().to_string(),
                    attributes: Vec::new(),
                });
            }
            Token::Attribute {
                prefix,
                local,
                value,
                ..
            } => {
                let element = pending
                    .as_mut()
                    .ok_or_else(|| validation("cloudfront_wire_xml_invalid"))?;
                element.attributes.push((
                    prefix.as_str().to_string(),
                    local.as_str().to_string(),
                    value.as_str().to_string(),
                ));
            }
            Token::ElementEnd { end, .. } => match end {
                ElementEnd::Open => {
                    let element = start_element(pending.take(), stack.len(), &mut nodes)?;
                    if stack.len() >= MAX_XML_DEPTH {
                        return Err(validation("cloudfront_wire_xml_depth_exceeded"));
                    }
                    stack.push(element);
                }
                ElementEnd::Empty => {
                    let element = start_element(pending.take(), stack.len(), &mut nodes)?;
                    attach_element(element, &mut stack, &mut root)?;
                }
                ElementEnd::Close(prefix, local) => {
                    if pending.is_some() || !prefix.as_str().is_empty() {
                        return Err(validation("cloudfront_wire_xml_namespace_mismatch"));
                    }
                    let element = stack
                        .pop()
                        .ok_or_else(|| validation("cloudfront_wire_xml_unbalanced"))?;
                    if element.name != local.as_str() {
                        return Err(validation("cloudfront_wire_xml_unbalanced"));
                    }
                    attach_element(element, &mut stack, &mut root)?;
                }
            },
            Token::Text { text } => {
                if let Some(element) = stack.last_mut() {
                    element.text.push_str(&decode_xml_text(text.as_str())?);
                } else if !text.as_str().chars().all(is_xml_whitespace) {
                    return Err(validation("cloudfront_wire_xml_text_outside_root"));
                }
            }
            Token::Comment { .. }
            | Token::ProcessingInstruction { .. }
            | Token::DtdStart { .. }
            | Token::EmptyDtd { .. }
            | Token::EntityDeclaration { .. }
            | Token::DtdEnd { .. }
            | Token::Cdata { .. } => {
                return Err(validation("cloudfront_wire_xml_unsupported_construct"));
            }
        }
    }
    if pending.is_some() || !stack.is_empty() {
        return Err(validation("cloudfront_wire_xml_unbalanced"));
    }
    let root = root.ok_or_else(|| validation("cloudfront_wire_xml_root_missing"))?;
    if root.name != "DistributionConfig" {
        return Err(validation("cloudfront_wire_xml_root_mismatch"));
    }
    Ok(root)
}

fn start_element(
    pending: Option<PendingElement>,
    depth: usize,
    nodes: &mut usize,
) -> CloudFrontApiResult<CanonicalElement> {
    let mut pending = pending.ok_or_else(|| validation("cloudfront_wire_xml_invalid"))?;
    if !pending.prefix.is_empty() {
        return Err(validation("cloudfront_wire_xml_namespace_mismatch"));
    }
    if depth == 0 {
        if pending.attributes.as_slice()
            != [(
                String::new(),
                "xmlns".to_string(),
                CLOUDFRONT_XML_NAMESPACE.to_string(),
            )]
        {
            return Err(validation("cloudfront_wire_xml_namespace_mismatch"));
        }
    } else if !pending.attributes.is_empty() {
        return Err(validation("cloudfront_wire_xml_attribute_unsupported"));
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_XML_NODES {
        return Err(validation("cloudfront_wire_xml_node_limit_exceeded"));
    }
    Ok(CanonicalElement {
        name: std::mem::take(&mut pending.name),
        text: String::new(),
        children: Vec::new(),
    })
}

fn attach_element(
    mut element: CanonicalElement,
    stack: &mut [CanonicalElement],
    root: &mut Option<CanonicalElement>,
) -> CloudFrontApiResult<()> {
    if !element.children.is_empty() {
        if !element.text.chars().all(is_xml_whitespace) {
            return Err(validation("cloudfront_wire_xml_mixed_content"));
        }
        element.text.zeroize();
        element.text.clear();
    }
    if let Some(parent) = stack.last_mut() {
        parent.children.push(element);
    } else if root.replace(element).is_some() {
        return Err(validation("cloudfront_wire_xml_multiple_roots"));
    }
    Ok(())
}

fn is_xml_whitespace(value: char) -> bool {
    matches!(value, ' ' | '\t' | '\r' | '\n')
}

fn decode_xml_text(value: &str) -> CloudFrontApiResult<String> {
    let mut decoded = Zeroizing::new(String::with_capacity(value.len()));
    let mut remaining = value;
    while let Some(index) = remaining.find('&') {
        decoded.push_str(&remaining[..index]);
        let entity = &remaining[index + 1..];
        let end = entity
            .find(';')
            .ok_or_else(|| validation("cloudfront_wire_xml_invalid_escape"))?;
        let entity = &entity[..end];
        match entity {
            "amp" => decoded.push('&'),
            "apos" => decoded.push('\''),
            "gt" => decoded.push('>'),
            "lt" => decoded.push('<'),
            "quot" => decoded.push('"'),
            value if value.starts_with("#x") => push_xml_codepoint(&mut decoded, &value[2..], 16)?,
            value if value.starts_with('#') => push_xml_codepoint(&mut decoded, &value[1..], 10)?,
            _ => return Err(validation("cloudfront_wire_xml_invalid_escape")),
        }
        remaining = &remaining[index + end + 2..];
    }
    decoded.push_str(remaining);
    Ok(std::mem::take(&mut *decoded))
}

fn push_xml_codepoint(output: &mut String, value: &str, radix: u32) -> CloudFrontApiResult<()> {
    let codepoint = u32::from_str_radix(value, radix)
        .ok()
        .filter(|value| is_xml_codepoint(*value))
        .and_then(char::from_u32)
        .ok_or_else(|| validation("cloudfront_wire_xml_invalid_escape"))?;
    output.push(codepoint);
    Ok(())
}

fn is_xml_codepoint(value: u32) -> bool {
    matches!(value, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn compare_documents(observed: &[u8], serialized: &[u8]) -> CloudFrontApiResult<()> {
    let observed = parse_document(observed)?;
    let serialized = parse_document(serialized)?;
    if observed != serialized {
        return Err(validation("cloudfront_wire_round_trip_mismatch"));
    }
    Ok(())
}

fn validate_root_enabled_only(current: &[u8], desired: &[u8]) -> CloudFrontApiResult<()> {
    let current = parse_document(current)?;
    let desired = parse_document(desired)?;
    if current.name != desired.name
        || current.text != desired.text
        || current.children.len() != desired.children.len()
    {
        return Err(validation("cloudfront_wire_write_set_mismatch"));
    }
    let mut root_enabled = 0usize;
    for (current, desired) in current.children.iter().zip(&desired.children) {
        if current.name == "Enabled" && desired.name == "Enabled" {
            root_enabled += 1;
            if !current.children.is_empty()
                || !desired.children.is_empty()
                || !matches!(current.text.as_str(), "true" | "false")
                || !matches!(desired.text.as_str(), "true" | "false")
            {
                return Err(validation("cloudfront_wire_enabled_invalid"));
            }
        } else if current != desired {
            return Err(validation("cloudfront_wire_write_set_mismatch"));
        }
    }
    if root_enabled != 1 {
        return Err(validation("cloudfront_wire_enabled_ambiguous"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_config::BehaviorVersion;
    use aws_credential_types::Credentials;
    use aws_sdk_cloudfront::config::Region;

    const NS: &str = "http://cloudfront.amazonaws.com/doc/2020-05-31/";

    fn document(inner: &str) -> String {
        format!(r#"<DistributionConfig xmlns="{NS}">{inner}</DistributionConfig>"#)
    }

    #[test]
    fn canonical_secret_fixture_ignores_only_formatting() {
        let observed = document(
            "\n<CallerReference>caller</CallerReference><Comment>a&amp;b</Comment>\n<Enabled>true</Enabled>",
        );
        let serialized = document(
            "<CallerReference>caller</CallerReference><Comment>a&#38;b</Comment><Enabled>true</Enabled>",
        );
        assert!(compare_documents(observed.as_bytes(), serialized.as_bytes()).is_ok());
    }

    #[test]
    fn unknown_top_and_deep_fields_fail_round_trip() {
        let serialized = document(
            "<CallerReference>caller</CallerReference><Origins><Quantity>0</Quantity><Items></Items></Origins><Comment></Comment><Enabled>true</Enabled>",
        );
        let top = document(
            "<CallerReference>caller</CallerReference><Future>secret</Future><Origins><Quantity>0</Quantity><Items></Items></Origins><Comment></Comment><Enabled>true</Enabled>",
        );
        let deep = document(
            "<CallerReference>caller</CallerReference><Origins><Quantity>0</Quantity><Future>secret</Future><Items></Items></Origins><Comment></Comment><Enabled>true</Enabled>",
        );
        assert_eq!(
            compare_documents(top.as_bytes(), serialized.as_bytes())
                .expect_err("unknown top")
                .code(),
            "cloudfront_wire_round_trip_mismatch"
        );
        assert_eq!(
            compare_documents(deep.as_bytes(), serialized.as_bytes())
                .expect_err("unknown deep")
                .code(),
            "cloudfront_wire_round_trip_mismatch"
        );
    }

    #[test]
    fn duplicate_enabled_is_rejected() {
        let duplicated = document("<Enabled>true</Enabled><Enabled>false</Enabled>");
        let current = document("<Enabled>false</Enabled>");
        assert_eq!(
            compare_documents(duplicated.as_bytes(), current.as_bytes())
                .expect_err("duplicate field")
                .code(),
            "cloudfront_wire_round_trip_mismatch"
        );
    }

    #[test]
    fn namespace_spoof_dtd_and_cdata_are_rejected() {
        let spoof = r#"<x:DistributionConfig xmlns:x="http://cloudfront.amazonaws.com/doc/2020-05-31/"><x:Enabled>true</x:Enabled></x:DistributionConfig>"#;
        let dtd = format!(
            r#"<!DOCTYPE DistributionConfig><DistributionConfig xmlns="{NS}"><Enabled>true</Enabled></DistributionConfig>"#
        );
        let cdata = document("<Comment><![CDATA[secret]]></Comment><Enabled>true</Enabled>");
        for value in [spoof.as_bytes(), dtd.as_bytes(), cdata.as_bytes()] {
            assert!(parse_document(value).is_err());
        }
    }

    #[test]
    fn desired_wire_may_change_only_the_direct_root_enabled() {
        let current =
            document("<Logging><Enabled>false</Enabled></Logging><Enabled>true</Enabled>");
        let desired =
            document("<Logging><Enabled>false</Enabled></Logging><Enabled>false</Enabled>");
        assert!(validate_root_enabled_only(current.as_bytes(), desired.as_bytes()).is_ok());

        let nested_changed =
            document("<Logging><Enabled>true</Enabled></Logging><Enabled>false</Enabled>");
        assert_eq!(
            validate_root_enabled_only(current.as_bytes(), nested_changed.as_bytes())
                .expect_err("nested change")
                .code(),
            "cloudfront_wire_write_set_mismatch"
        );
    }

    #[tokio::test]
    async fn oversized_serialized_request_fails_before_capture_or_transmit() {
        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(Credentials::new("key", "secret", None, None, "test"))
            .region(Region::new("us-east-1"))
            .load()
            .await;
        let client = aws_sdk_cloudfront::Client::new(&sdk_config);
        let config = DistributionConfig::builder()
            .caller_reference("caller-reference")
            .comment("x".repeat(MAX_MUTATION_REQUEST_BYTES))
            .enabled(false)
            .build()
            .expect("generated SDK config");

        let Err(error) =
            serialize_without_transmit(&client, "E123EXAMPLE", "E2DETAIL", config).await
        else {
            panic!("oversized request must fail closed")
        };
        assert_eq!(error.code(), "cloudfront_wire_request_too_large");
    }
}
