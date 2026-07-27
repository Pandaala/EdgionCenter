//! Narrow HTTP-over-federation client contract.
//!
//! Defines [`ControllerHttpClient`] and its response type: the runtime-side
//! abstraction for issuing an Admin API request against a connected Controller
//! through the federation proxy tunnel. The production implementation is
//! `ProxyForwarder` in `crate::proxy`.

use std::collections::HashMap;

use edgion_center_core::ControllerOwnerRoute;

/// HTTP response returned through a connected Controller.
pub struct ControllerHttpResponse {
    pub status_code: u32,
    pub body: Vec<u8>,
}

/// Narrow runtime contract for Controller-bound HTTP requests.
#[async_trait::async_trait]
pub trait ControllerHttpClient: Send + Sync {
    async fn request(
        &self,
        controller_id: &str,
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    ) -> Result<ControllerHttpResponse, String>;

    /// Execute against exactly the supplied owner route and fence. Implementations
    /// must not re-resolve, retry on a newer owner, or fall back to an unfenced
    /// local session.
    async fn request_fenced(
        &self,
        _controller_id: &str,
        _method: String,
        _path: String,
        _headers: HashMap<String, String>,
        _body: Vec<u8>,
        _expected_owner: &ControllerOwnerRoute,
    ) -> Result<ControllerHttpResponse, String> {
        Err("owner-fenced Controller request is unsupported".to_string())
    }

    /// Execute only while the supplied standalone session remains current.
    async fn request_session_fenced(
        &self,
        _controller_id: &str,
        _method: String,
        _path: String,
        _headers: HashMap<String, String>,
        _body: Vec<u8>,
        _expected_session_id: &str,
    ) -> Result<ControllerHttpResponse, String> {
        Err("session-fenced Controller request is unsupported".to_string())
    }
}
