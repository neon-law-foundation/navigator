//! Kubernetes containment for isolated GitHub engineering runners.
//!
//! This is the sole Rust crate that speaks Kubernetes for `DevX` runner jobs.

use async_trait::async_trait;
use k8s_openapi::{
    api::core::v1::{Namespace, ResourceQuota, ResourceQuotaSpec},
    apimachinery::pkg::api::resource::Quantity,
    apimachinery::pkg::apis::meta::v1::ObjectMeta,
};
use kube::{
    api::{Api, DeleteParams, PostParams},
    Client,
};
use std::collections::BTreeMap;
use thiserror::Error;

/// An isolated runner namespace identifier containing delivery metadata only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerNamespace(String);

impl RunnerNamespace {
    /// Validate one controller-generated Kubernetes namespace name.
    pub fn new(value: String) -> Result<Self, ClusterError> {
        if value.is_empty()
            || value.len() > 63
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || value.starts_with('-')
            || value.ends_with('-')
        {
            return Err(ClusterError::InvalidNamespace);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Source-safe Kubernetes lifecycle failures.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ClusterError {
    #[error("runner namespace is invalid")]
    InvalidNamespace,
    #[error("runner namespace operation failed")]
    Unavailable,
}

/// The only Kubernetes boundary durable GitHub workflows may use.
#[async_trait]
pub trait ClusterOps: Send + Sync {
    async fn provision(&self, namespace: &RunnerNamespace) -> Result<(), ClusterError>;
    async fn teardown(&self, namespace: &RunnerNamespace) -> Result<(), ClusterError>;
}

/// Native Kubernetes implementation used by the durable worker.
///
/// Runner namespaces are the only namespaces this client creates or deletes.
/// The Job's node selector and credential projection are deliberately owned by
/// the subsequent Job controller, never by `workflows-service` itself.
#[derive(Clone)]
pub struct KubernetesClusterOps {
    client: Client,
}

impl KubernetesClusterOps {
    #[must_use]
    pub const fn new(client: Client) -> Self {
        Self { client }
    }

    fn quota(namespace: &RunnerNamespace) -> ResourceQuota {
        let mut hard = BTreeMap::new();
        for (name, value) in [
            ("requests.cpu", "4"),
            ("limits.cpu", "4"),
            ("requests.memory", "16Gi"),
            ("limits.memory", "16Gi"),
            ("pods", "1"),
            ("count/jobs.batch", "1"),
        ] {
            hard.insert(name.to_owned(), Quantity(value.to_owned()));
        }
        ResourceQuota {
            metadata: ObjectMeta {
                name: Some("github-runner".into()),
                namespace: Some(namespace.0.clone()),
                ..ObjectMeta::default()
            },
            spec: Some(ResourceQuotaSpec {
                hard: Some(hard),
                ..ResourceQuotaSpec::default()
            }),
            ..ResourceQuota::default()
        }
    }
}

#[async_trait]
impl ClusterOps for KubernetesClusterOps {
    async fn provision(&self, namespace: &RunnerNamespace) -> Result<(), ClusterError> {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());
        if namespaces
            .get_opt(namespace.as_str())
            .await
            .map_err(|_| ClusterError::Unavailable)?
            .is_none()
        {
            namespaces
                .create(
                    &PostParams::default(),
                    &Namespace {
                        metadata: ObjectMeta {
                            name: Some(namespace.0.clone()),
                            ..ObjectMeta::default()
                        },
                        ..Namespace::default()
                    },
                )
                .await
                .map_err(|_| ClusterError::Unavailable)?;
        }
        let quotas: Api<ResourceQuota> = Api::namespaced(self.client.clone(), namespace.as_str());
        if quotas
            .get_opt("github-runner")
            .await
            .map_err(|_| ClusterError::Unavailable)?
            .is_none()
        {
            quotas
                .create(&PostParams::default(), &Self::quota(namespace))
                .await
                .map_err(|_| ClusterError::Unavailable)?;
        }
        Ok(())
    }

    async fn teardown(&self, namespace: &RunnerNamespace) -> Result<(), ClusterError> {
        let namespaces: Api<Namespace> = Api::all(self.client.clone());
        if namespaces
            .get_opt(namespace.as_str())
            .await
            .map_err(|_| ClusterError::Unavailable)?
            .is_some()
        {
            namespaces
                .delete(namespace.as_str(), &DeleteParams::default())
                .await
                .map_err(|_| ClusterError::Unavailable)?;
        }
        Ok(())
    }
}

/// Delivery-keyed, content-free namespace derivation.
pub fn namespace_for_delivery(delivery_id: &str) -> Result<RunnerNamespace, ClusterError> {
    let suffix: String = delivery_id
        .bytes()
        .filter(u8::is_ascii_hexdigit)
        .map(|byte| byte.to_ascii_lowercase() as char)
        .take(32)
        .collect();
    RunnerNamespace::new(format!("github-runner-{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::{namespace_for_delivery, RunnerNamespace};

    #[test]
    fn delivery_namespace_is_bounded_and_content_free() {
        let namespace = namespace_for_delivery("11111111-1111-4111-8111-111111111111").unwrap();
        assert_eq!(
            namespace.as_str(),
            "github-runner-11111111111141118111111111111111"
        );
    }

    #[test]
    fn namespace_rejects_non_kubernetes_names() {
        assert!(RunnerNamespace::new("Runner".into()).is_err());
        assert!(RunnerNamespace::new("-runner".into()).is_err());
    }
}
