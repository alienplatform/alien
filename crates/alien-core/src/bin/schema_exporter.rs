use alien_core::*;
use clap::Parser;
use std::{fs::File, io::Write as _};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(components(schemas(
    Function,
    FunctionOutputs,
    Container,
    ContainerOutputs,
    ContainerCode,
    ContainerGpuSpec,
    ContainerAutoscaling,
    ContainerPort,
    ExposeProtocol,
    PersistentStorage,
    HealthCheck,
    ResourceSpec,
    ReplicaStatus,
    ContainerStatus,
    LoadBalancerEndpoint,
    Resource,
    ResourceLifecycle,
    ResourceOutputs,
    ResourceRef,
    ResourceStatus,
    ResourceType,
    Stack,
    StackRef,
    StackState,
    StackStatus,
    StackResourceState,
    StackSettings,
    NetworkSettings,
    DomainSettings,
    CustomDomainConfig,
    CustomCertificateConfig,
    AwsCustomCertificateConfig,
    GcpCustomCertificateConfig,
    AzureCustomCertificateConfig,
    ManagementConfig,
    AwsManagementConfig,
    GcpManagementConfig,
    AzureManagementConfig,
    ImagePullCredentials,
    Storage,
    StorageOutputs,
    Build,
    BuildOutputs,
    BuildStatus,
    BuildConfig,
    ArtifactRegistry,
    ArtifactRegistryOutputs,
    ServiceAccount,
    ServiceAccountOutputs,
    RemoteStackManagement,
    RemoteStackManagementOutputs,
    Vault,
    VaultOutputs,
    Kv,
    KvOutputs,
    Queue,
    QueueOutputs,
    PlatformPermissions,
    PermissionSet,
    PermissionSetReference,
    PermissionProfile,
    ManagementPermissions,
    AlienEvent,
    EventChange,
    DeploymentModel,
    UpdatesMode,
    TelemetryMode,
    HeartbeatsMode,
    // App events (for function triggers)
    StorageEvent,
    StorageEventType,
    StorageEvents,
    QueueMessage,
    MessagePayload,
    ScheduledEvent,
    // Dev status types (for alien dev --status-file output)
    DevStatus,
    DevStatusState,
    AgentStatus,
    DevResourceInfo,
    // ARC protocol types
    CommandState,
    BodySpec,
    CommandResponse,
    ResponseHandling,
    Envelope,
    CreateCommandRequest,
    StorageUpload,
    CreateCommandResponse,
    UploadCompleteRequest,
    UploadCompleteResponse,
    CommandStatusResponse,
    SubmitResponseRequest,
    LeaseRequest,
    LeaseInfo,
    LeaseResponse,
    ReleaseRequest,
)))]
struct ApiDoc;

/// A simple program to export OpenAPI spec
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Output path for the OpenAPI JSON file
    #[arg(short, long, default_value = "openapi.json")]
    output: String,
}

fn main() {
    let args = Args::parse();

    let mut file = File::create(&args.output).unwrap();
    file.write_all(ApiDoc::openapi().to_pretty_json().unwrap().as_bytes())
        .unwrap();
    println!("OpenAPI spec exported to {}", args.output);
}
