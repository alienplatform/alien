//! Selected ECS request and response fields used by durable service owners.
//! https://docs.aws.amazon.com/AmazonECS/latest/APIReference/Welcome.html

use bon::Builder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateClusterRequest {
    pub cluster_name: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DescribeClustersRequest {
    pub clusters: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ClusterRequest {
    pub cluster: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RegisterTaskDefinitionRequest {
    pub family: String,
    pub container_definitions: Vec<ContainerDefinition>,
    pub network_mode: String,
    pub requires_compatibilities: Vec<String>,
    pub cpu: String,
    pub memory: String,
    pub runtime_platform: RuntimePlatform,
    pub tags: Vec<Tag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_role_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_role_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DescribeTaskDefinitionRequest {
    pub task_definition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinitionRequest {
    pub task_definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskDefinitionsRequest {
    pub family_prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTaskDefinitionsRequest {
    pub task_definitions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceRequest {
    pub cluster: String,
    pub service_name: String,
    pub client_token: String,
    pub task_definition: String,
    pub desired_count: i32,
    pub launch_type: String,
    pub platform_version: String,
    pub network_configuration: NetworkConfiguration,
    pub load_balancers: Vec<LoadBalancer>,
    pub deployment_configuration: DeploymentConfiguration,
    pub health_check_grace_period_seconds: i32,
    pub tags: Vec<Tag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub propagate_tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_ecs_managed_tags: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UpdateServiceRequest {
    pub cluster: String,
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_definition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_configuration: Option<DeploymentConfiguration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DescribeServicesRequest {
    pub cluster: String,
    pub services: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeleteServiceRequest {
    pub cluster: String,
    pub service: String,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksRequest {
    pub cluster: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DescribeTasksRequest {
    pub cluster: String,
    pub tasks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ContainerDefinition {
    pub name: String,
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub essential: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_mappings: Option<Vec<PortMapping>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_timeout: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Vec<KeyValuePair>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly_root_filesystem: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PortMapping {
    pub container_port: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct KeyValuePair {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePlatform {
    pub cpu_architecture: String,
    pub operating_system_family: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfiguration {
    pub awsvpc_configuration: AwsVpcConfiguration,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AwsVpcConfiguration {
    pub subnets: Vec<String>,
    pub security_groups: Vec<String>,
    pub assign_public_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancer {
    pub target_group_arn: String,
    pub container_name: String,
    pub container_port: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_percent: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_healthy_percent: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_circuit_breaker: Option<DeploymentCircuitBreaker>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentCircuitBreaker {
    pub enable: bool,
    pub rollback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterResponse {
    pub cluster: Cluster,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeClustersResponse {
    #[serde(default)]
    pub clusters: Vec<Cluster>,
    #[serde(default)]
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    pub cluster_arn: String,
    pub cluster_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_services_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_tasks_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_tasks_count: Option<i32>,
    #[serde(default)]
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinitionResponse {
    pub task_definition: TaskDefinition,
    #[serde(default)]
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    pub task_definition_arn: String,
    pub family: String,
    pub revision: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub container_definitions: Vec<ContainerDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_platform: Option<RuntimePlatform>,
    #[serde(default)]
    pub requires_compatibilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_role_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_role_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskDefinitionsResponse {
    #[serde(default)]
    pub task_definition_arns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTaskDefinitionsResponse {
    #[serde(default)]
    pub task_definitions: Vec<TaskDefinition>,
    #[serde(default)]
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceResponse {
    pub service: Service,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeServicesResponse {
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default)]
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub service_arn: String,
    pub service_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_definition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_configuration: Option<NetworkConfiguration>,
    #[serde(default)]
    pub load_balancers: Vec<LoadBalancer>,
    #[serde(default)]
    pub deployments: Vec<Deployment>,
    #[serde(default)]
    pub events: Vec<ServiceEvent>,
    #[serde(default)]
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_definition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollout_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollout_state_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResponse {
    #[serde(default)]
    pub task_arns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeTasksResponse {
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub task_arn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_definition_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_reason: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub containers: Vec<TaskContainer>,
    #[serde(default)]
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub details: Vec<KeyValuePair>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContainer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
