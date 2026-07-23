//! Low-cardinality telemetry for retained cloud Admin APIs.

use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};
use edgion_center_core::ProviderErrorCategory;
use metrics::{counter, gauge, histogram};

pub const CLOUD_REQUESTS_TOTAL: &str = "edgion_center_cloud_requests_total";
pub const CLOUD_REQUEST_DURATION_SECONDS: &str = "edgion_center_cloud_request_duration_seconds";
pub const CLOUD_REQUESTS_IN_FLIGHT: &str = "edgion_center_cloud_requests_in_flight";
pub const CLOUD_PROVIDER_ERRORS_TOTAL: &str = "edgion_center_cloud_provider_errors_total";
pub const CLOUD_PROVIDER_THROTTLED_TOTAL: &str = "edgion_center_cloud_provider_throttled_total";

fn route_labels(path: &str) -> Option<(&'static str, &'static str)> {
    if path.starts_with("/api/v1/center/cloudflare/dns/") {
        Some(("cloudflare", "dns"))
    } else if path.starts_with("/api/v1/center/cloudflare/waf/") {
        Some(("cloudflare", "waf"))
    } else if path.starts_with("/api/v1/center/aws/route53/") {
        Some(("aws", "dns"))
    } else if path.starts_with("/api/v1/center/aws/cloudfront/") {
        Some(("aws", "cloudfront"))
    } else if path.starts_with("/api/v1/center/aws/waf/") {
        Some(("aws", "waf"))
    } else if path.starts_with("/api/v1/center/cloud/provider-") {
        Some(("center", "provider_account"))
    } else {
        None
    }
}

/// Records end-to-end Admin API latency and status without account/resource labels.
pub async fn cloud_metrics_middleware(request: Request, next: Next) -> Response {
    let Some((provider, service)) = route_labels(request.uri().path()) else {
        return next.run(request).await;
    };
    let method = match request.method().as_str() {
        "GET" | "HEAD" => "read",
        _ => "write",
    };
    let in_flight = gauge!(
        CLOUD_REQUESTS_IN_FLIGHT,
        "provider" => provider,
        "service" => service,
        "method" => method
    );
    in_flight.increment(1.0);
    let started = Instant::now();
    let response = next.run(request).await;
    in_flight.decrement(1.0);

    let outcome = if response.status().is_success() {
        "success"
    } else if response.status().is_client_error() {
        "client_error"
    } else {
        "provider_error"
    };
    counter!(
        CLOUD_REQUESTS_TOTAL,
        "provider" => provider,
        "service" => service,
        "method" => method,
        "outcome" => outcome
    )
    .increment(1);
    histogram!(
        CLOUD_REQUEST_DURATION_SECONDS,
        "provider" => provider,
        "service" => service,
        "method" => method
    )
    .record(started.elapsed().as_secs_f64());
    response
}

/// Records the normalized category before provider-specific services reduce it
/// to their intentionally small public error enum.
pub fn record_provider_error(provider: &'static str, category: ProviderErrorCategory) {
    let category = match category {
        ProviderErrorCategory::Authentication => "authentication",
        ProviderErrorCategory::Authorization => "authorization",
        ProviderErrorCategory::Quota => "quota",
        ProviderErrorCategory::Throttled => "throttled",
        ProviderErrorCategory::Conflict => "conflict",
        ProviderErrorCategory::Validation => "validation",
        ProviderErrorCategory::NotFound => "not_found",
        ProviderErrorCategory::Transient => "transient",
        ProviderErrorCategory::UnknownOutcome => "unknown_outcome",
    };
    counter!(
        CLOUD_PROVIDER_ERRORS_TOTAL,
        "provider" => provider,
        "category" => category
    )
    .increment(1);
    if category == "throttled" {
        counter!(CLOUD_PROVIDER_THROTTLED_TOTAL, "provider" => provider).increment(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_labels_are_bounded_and_never_include_resource_ids() {
        assert_eq!(
            route_labels("/api/v1/center/aws/cloudfront/accounts/123/distributions/secret/origin"),
            Some(("aws", "cloudfront"))
        );
        assert_eq!(
            route_labels("/api/v1/center/cloudflare/waf/accounts/a/zones/z/rulesets"),
            Some(("cloudflare", "waf"))
        );
        assert_eq!(route_labels("/api/v1/controllers"), None);
    }
}
