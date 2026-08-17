//! Pingora integration for the policy in [`crate`].

use std::net::IpAddr;

use async_trait::async_trait;
use pingora::http::{RequestHeader, ResponseHeader};
use pingora::proxy::{ProxyHttp, Session};
use pingora::upstreams::peer::HttpPeer;

use crate::{decide, Decision, GatewayConfig};

const REALM: &str = "Basic realm=\"Neon Law Navigator (private)\"";

/// The private-mode sidecar service.
pub struct PrivateGateway {
    config: GatewayConfig,
}

impl PrivateGateway {
    #[must_use]
    pub fn new(config: GatewayConfig) -> Self {
        Self { config }
    }
}

fn peer_ip(session: &Session) -> Option<IpAddr> {
    session
        .client_addr()
        .and_then(pingora::protocols::l4::socket::SocketAddr::as_inet)
        .map(std::net::SocketAddr::ip)
}

fn header<'a>(request: &'a RequestHeader, name: &str) -> Option<&'a str> {
    request
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
}

#[async_trait]
impl ProxyHttp for PrivateGateway {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<bool> {
        let request = session.req_header();
        match decide(
            &self.config,
            request.uri.path(),
            peer_ip(session),
            header(request, "x-forwarded-for"),
            header(request, "authorization"),
        ) {
            Decision::Proxy => Ok(false),
            Decision::Forbidden => {
                session.respond_error(403).await?;
                Ok(true)
            }
            Decision::Unauthorized => {
                let mut response = ResponseHeader::build(401, None)?;
                response.insert_header("WWW-Authenticate", REALM)?;
                response.insert_header("Content-Length", "0")?;
                session
                    .write_response_header(Box::new(response), true)
                    .await?;
                Ok(true)
            }
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<Box<HttpPeer>> {
        Ok(Box::new(HttpPeer::new(
            self.config.upstream,
            false,
            String::new(),
        )))
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        if let Some(peer) = peer_ip(session) {
            let forwarded = match header(session.req_header(), "x-forwarded-for") {
                Some(existing) => format!("{existing}, {peer}"),
                None => peer.to_string(),
            };
            upstream.insert_header("X-Forwarded-For", forwarded)?;
            upstream.insert_header("X-Real-IP", peer.to_string())?;
        }
        Ok(())
    }
}
