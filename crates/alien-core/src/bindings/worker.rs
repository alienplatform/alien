//! Worker binding definitions for cross-worker communication
//!
//! This module defines the binding parameters for worker invocation services:
//! - AWS Lambda (using worker ARN/name for direct invocation)
//! - GCP Cloud Run (using private service URL)
//! - Azure Container Apps (using private container app URL)

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Represents a worker binding for cross-worker communication
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum WorkerBinding {
    /// AWS Lambda binding
    Lambda(LambdaWorkerBinding),
    /// GCP Cloud Run binding
    CloudRun(CloudRunWorkerBinding),
    /// Azure Container Apps binding
    ContainerApp(ContainerAppWorkerBinding),
    /// Kubernetes worker binding
    Kubernetes(KubernetesWorkerBinding),
    /// Local worker binding
    Local(LocalWorkerBinding),
}

/// AWS Lambda worker binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LambdaWorkerBinding {
    /// The Lambda worker name or ARN for invocation
    pub worker_name: BindingValue<String>,
    /// The AWS region where the worker is located
    pub region: BindingValue<String>,
    /// Optional public URL if worker has public ingress
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<BindingValue<String>>,
}

/// GCP Cloud Run worker binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRunWorkerBinding {
    /// The GCP project ID
    pub project_id: BindingValue<String>,
    /// The Cloud Run service name
    pub service_name: BindingValue<String>,
    /// The location/region where the service is deployed
    pub location: BindingValue<String>,
    /// Private service URL for direct invocation
    pub private_url: BindingValue<String>,
    /// Optional public URL if worker has public ingress
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<BindingValue<String>>,
}

/// Azure Container Apps worker binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerAppWorkerBinding {
    /// The Azure subscription ID
    pub subscription_id: BindingValue<String>,
    /// The resource group name
    pub resource_group_name: BindingValue<String>,
    /// The container app name
    pub container_app_name: BindingValue<String>,
    /// Private app URL for direct invocation within managed environment
    pub private_url: BindingValue<String>,
    /// Optional public URL if worker has public ingress
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<BindingValue<String>>,
}

/// Kubernetes worker binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesWorkerBinding {
    /// The worker name
    pub name: BindingValue<String>,
    /// The Kubernetes namespace
    pub namespace: BindingValue<String>,
    /// The Kubernetes Service name
    pub service_name: BindingValue<String>,
    /// The Service port
    pub service_port: BindingValue<u16>,
    /// Optional public URL if worker has public ingress
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<BindingValue<String>>,
}

/// Local worker binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalWorkerBinding {
    /// The HTTP URL where the worker is accessible
    pub worker_url: BindingValue<String>,
}

impl WorkerBinding {
    /// Creates an AWS Lambda worker binding
    pub fn lambda(
        worker_name: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Lambda(LambdaWorkerBinding {
            worker_name: worker_name.into(),
            region: region.into(),
            url: None,
        })
    }

    /// Creates an AWS Lambda worker binding with public URL
    pub fn lambda_with_url(
        worker_name: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
        url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Lambda(LambdaWorkerBinding {
            worker_name: worker_name.into(),
            region: region.into(),
            url: Some(url.into()),
        })
    }

    /// Creates a GCP Cloud Run worker binding
    pub fn cloud_run(
        project_id: impl Into<BindingValue<String>>,
        service_name: impl Into<BindingValue<String>>,
        location: impl Into<BindingValue<String>>,
        private_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::CloudRun(CloudRunWorkerBinding {
            project_id: project_id.into(),
            service_name: service_name.into(),
            location: location.into(),
            private_url: private_url.into(),
            public_url: None,
        })
    }

    /// Creates a GCP Cloud Run worker binding with public URL
    pub fn cloud_run_with_public_url(
        project_id: impl Into<BindingValue<String>>,
        service_name: impl Into<BindingValue<String>>,
        location: impl Into<BindingValue<String>>,
        private_url: impl Into<BindingValue<String>>,
        public_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::CloudRun(CloudRunWorkerBinding {
            project_id: project_id.into(),
            service_name: service_name.into(),
            location: location.into(),
            private_url: private_url.into(),
            public_url: Some(public_url.into()),
        })
    }

    /// Creates an Azure Container Apps worker binding
    pub fn container_app(
        subscription_id: impl Into<BindingValue<String>>,
        resource_group_name: impl Into<BindingValue<String>>,
        container_app_name: impl Into<BindingValue<String>>,
        private_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::ContainerApp(ContainerAppWorkerBinding {
            subscription_id: subscription_id.into(),
            resource_group_name: resource_group_name.into(),
            container_app_name: container_app_name.into(),
            private_url: private_url.into(),
            public_url: None,
        })
    }

    /// Creates an Azure Container Apps worker binding with public URL
    pub fn container_app_with_public_url(
        subscription_id: impl Into<BindingValue<String>>,
        resource_group_name: impl Into<BindingValue<String>>,
        container_app_name: impl Into<BindingValue<String>>,
        private_url: impl Into<BindingValue<String>>,
        public_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::ContainerApp(ContainerAppWorkerBinding {
            subscription_id: subscription_id.into(),
            resource_group_name: resource_group_name.into(),
            container_app_name: container_app_name.into(),
            private_url: private_url.into(),
            public_url: Some(public_url.into()),
        })
    }

    /// Creates a local worker binding
    pub fn local(worker_url: impl Into<BindingValue<String>>) -> Self {
        Self::Local(LocalWorkerBinding {
            worker_url: worker_url.into(),
        })
    }

    /// Creates a Kubernetes worker binding
    pub fn kubernetes(
        name: impl Into<BindingValue<String>>,
        namespace: impl Into<BindingValue<String>>,
        service_name: impl Into<BindingValue<String>>,
        service_port: impl Into<BindingValue<u16>>,
    ) -> Self {
        Self::Kubernetes(KubernetesWorkerBinding {
            name: name.into(),
            namespace: namespace.into(),
            service_name: service_name.into(),
            service_port: service_port.into(),
            public_url: None,
        })
    }

    /// Creates a Kubernetes worker binding with public URL
    pub fn kubernetes_with_public_url(
        name: impl Into<BindingValue<String>>,
        namespace: impl Into<BindingValue<String>>,
        service_name: impl Into<BindingValue<String>>,
        service_port: impl Into<BindingValue<u16>>,
        public_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Kubernetes(KubernetesWorkerBinding {
            name: name.into(),
            namespace: namespace.into(),
            service_name: service_name.into(),
            service_port: service_port.into(),
            public_url: Some(public_url.into()),
        })
    }

    /// Gets the public URL if available for any platform
    pub fn get_public_url(&self) -> Option<&BindingValue<String>> {
        match self {
            WorkerBinding::Lambda(binding) => binding.url.as_ref(),
            WorkerBinding::CloudRun(binding) => binding.public_url.as_ref(),
            WorkerBinding::ContainerApp(binding) => binding.public_url.as_ref(),
            WorkerBinding::Kubernetes(binding) => binding.public_url.as_ref(),
            WorkerBinding::Local(binding) => Some(&binding.worker_url),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lambda_binding() {
        let binding = WorkerBinding::lambda("my-worker", "us-east-1");

        if let WorkerBinding::Lambda(lambda_binding) = binding {
            assert_eq!(
                lambda_binding.worker_name,
                BindingValue::Value("my-worker".to_string())
            );
            assert_eq!(
                lambda_binding.region,
                BindingValue::Value("us-east-1".to_string())
            );
            assert!(lambda_binding.url.is_none());
        } else {
            panic!("Expected Lambda binding");
        }
    }

    #[test]
    fn test_lambda_binding_with_url() {
        let binding = WorkerBinding::lambda_with_url(
            "my-worker",
            "us-east-1",
            "https://abc123.lambda-url.us-east-1.on.aws/",
        );

        if let WorkerBinding::Lambda(lambda_binding) = binding {
            assert_eq!(
                lambda_binding.worker_name,
                BindingValue::Value("my-worker".to_string())
            );
            assert_eq!(
                lambda_binding.region,
                BindingValue::Value("us-east-1".to_string())
            );
            assert_eq!(
                lambda_binding.url,
                Some(BindingValue::Value(
                    "https://abc123.lambda-url.us-east-1.on.aws/".to_string()
                ))
            );
        } else {
            panic!("Expected Lambda binding");
        }
    }

    #[test]
    fn test_cloud_run_binding() {
        let binding = WorkerBinding::cloud_run(
            "my-project",
            "my-service",
            "us-central1",
            "https://my-service-abc123.a.run.app",
        );

        if let WorkerBinding::CloudRun(cloudrun_binding) = binding {
            assert_eq!(
                cloudrun_binding.project_id,
                BindingValue::Value("my-project".to_string())
            );
            assert_eq!(
                cloudrun_binding.service_name,
                BindingValue::Value("my-service".to_string())
            );
            assert_eq!(
                cloudrun_binding.location,
                BindingValue::Value("us-central1".to_string())
            );
            assert_eq!(
                cloudrun_binding.private_url,
                BindingValue::Value("https://my-service-abc123.a.run.app".to_string())
            );
            assert!(cloudrun_binding.public_url.is_none());
        } else {
            panic!("Expected CloudRun binding");
        }
    }

    #[test]
    fn test_container_app_binding() {
        let binding = WorkerBinding::container_app(
            "sub-123",
            "my-rg",
            "my-app",
            "https://my-app.internal.env.region.azurecontainerapps.io",
        );

        if let WorkerBinding::ContainerApp(container_app_binding) = binding {
            assert_eq!(
                container_app_binding.subscription_id,
                BindingValue::Value("sub-123".to_string())
            );
            assert_eq!(
                container_app_binding.resource_group_name,
                BindingValue::Value("my-rg".to_string())
            );
            assert_eq!(
                container_app_binding.container_app_name,
                BindingValue::Value("my-app".to_string())
            );
            assert_eq!(
                container_app_binding.private_url,
                BindingValue::Value(
                    "https://my-app.internal.env.region.azurecontainerapps.io".to_string()
                )
            );
            assert!(container_app_binding.public_url.is_none());
        } else {
            panic!("Expected ContainerApp binding");
        }
    }

    #[test]
    fn test_binding_value_expressions() {
        use serde_json::json;

        let binding = WorkerBinding::Lambda(LambdaWorkerBinding {
            worker_name: BindingValue::Expression(json!({"Fn::Ref": "MyWorker"})),
            region: BindingValue::Value("us-east-1".to_string()),
            url: Some(BindingValue::Expression(
                json!({"Fn::GetAtt": ["MyWorker", "WorkerUrl"]}),
            )),
        });

        let serialized = serde_json::to_string(&binding).unwrap();
        let deserialized: WorkerBinding = serde_json::from_str(&serialized).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_get_public_url() {
        let lambda_binding = WorkerBinding::lambda_with_url(
            "my-worker",
            "us-east-1",
            "https://abc123.lambda-url.us-east-1.on.aws/",
        );
        assert!(lambda_binding.get_public_url().is_some());

        let lambda_binding_no_url = WorkerBinding::lambda("my-worker", "us-east-1");
        assert!(lambda_binding_no_url.get_public_url().is_none());
    }

    #[test]
    fn test_local_binding() {
        let binding = WorkerBinding::local("http://localhost:3000");

        if let WorkerBinding::Local(local_binding) = binding {
            assert_eq!(
                local_binding.worker_url,
                BindingValue::Value("http://localhost:3000".to_string())
            );
        } else {
            panic!("Expected Local binding");
        }
    }

    #[test]
    fn test_local_binding_public_url() {
        let binding = WorkerBinding::local("http://localhost:3000");
        let url = binding.get_public_url();
        assert!(url.is_some());
        assert_eq!(
            url.unwrap(),
            &BindingValue::Value("http://localhost:3000".to_string())
        );
    }
}
