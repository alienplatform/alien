# `alien-infra` - Design Document

## 1. Requirements

`alien-infra` is an opinionated Terraform alternative built for Alien, with some important differences:

0. `alien-infra` is fully serverless, multi-tenant, infinitely scalable, and you don't pay for compute when you're waiting for cloud resources to be deployed.

   It's adapted for millions of deployments per second which is what AI Coding Agents need.

   It lets you bring your own durable execution orchestrator (Temporal, Inngest, Restate, Trigger.dev, etc.).

1. Developers define stacks of cloud-agnostic resources like `Function`, `Storage`, and `Queue`.
   These can be automatically translated to specific resources like Lambda on AWS, Cloud Run on GCP, and so on.

   The supported platforms are: AWS, GCP, Azure, Kubernetes, Local.


2. Each resource is marked as either `frozen` or `live`.

   🧊 Frozen resources:
     * Created once during setup and are rarely (if ever) modified. 
     * Require less permissions after initial setup
     * Examples: S3 bucket created to store logs or backups, VPC, IAM permissions.
   
   🔁 Live resources:
     * Frequently updated as part of ongoing deployments
     * Require more permissions after initial setup, but allow frequent updates (e.g. redeploying code)
     * Example: Lambda function or Cloud Run service that changes as you ship new code

   Each resource also has a `initialSetup` flag to mark whether it should be deployed as part of initial setup or not. 
   
   Frozen resources **must** be deployed during initial setup, live resources by default are usually **not** deployed during initial setup,
   but this can be enabled optionally (useful in some scenarios like **Managing Functions** - see requirement #6).


3. The library can automatically derive the **Live Permissions** of a stack from its definition by platform (AWS, GCP, Azure, ...). 

   The **Live Permissions** of a stack are defined as a list of policies for each resource in the stack, where each policy includes:
     * Permissions necessary to create, update, read, and delete live resources with `initialSetup = false`
     * Permissions necessary to read and update live resources with `initialSetup = true`
     * Permissions necessary to monitor frozen resources
   
   For example, if we have a live `Function` resource, then on AWS, the `CreateFunction`, `UpdateFunctionCode`, etc. permissions 
   will be included in the Live Permissions (in a policy specific for that Lambda function).

   The purpose of Live Permissions is to enable least-privilege remote management of the stack.


4. For initial setup, the stack can be compiled to a CloudFormation or Azure ARM template.
   
   A user who has elevated permissions necessary for the initial deployment can then deploy this template.

   This will deploy all resources with the `initialSetup` flag (e.g. VPC, S3 bucket), and IAM permissions necessary to create and update live resources (e.g. Lambda function).

   Note: For Google Cloud we don't need to generate a template because deployment happens completely dynamically ("Login with Google"), as opposed to AWS and Azure
   where the user _must_ deploy something first.


5. Live permissions are given either to a **Managing Cloud Account** or a **Managing Function** resource.

   If live permissions are given to a managing cloud account, then management of the stack happens from that account using
   `AssumeRole` on AWS, service account impersonation on GCP, etc. This is only available on platforms like AWS, Google Cloud, and Azure.
   
   Any details about the managing cloud account are injected in run-time, not in the stack definition.
   In the stack definition, the developer simply marks the stack to be managed by a certain managing account.
  
   If live permissions are given to a Function resource, then the management of the stack happens from that function.
   The function can receive management (e.g. update) via any `alien-runtime` transport like http polling.
   The managing function must have `initialSetup = true`.


## 2. Architecture

The core component is the `alien-infra` Rust library.

It offers flexibility in deployment:
*   **Embedded:** Can be embedded directly into agents for self-update capabilities.
*   **Remote Management:** Can be used by the manager for remote updates of deployment infrastructure.
*   **CLI:** Provides a command-line interface (e.g., `main.rs`) for direct interaction.

Furthermore, `alien-infra` is designed for cross-language use:
*   **WASM:** Compiles to WebAssembly, enabling a TypeScript SDK for defining, compiling, and executing stacks from JavaScript/TypeScript environments.
*   **PyO3:** Uses PyO3 bindings to create a Python SDK with similar capabilities for Python developers.

This unified Rust core simplifies development and testing across different deployment scenarios and language interfaces.

The library provides:
*   Stack definition types (`Function`, `Storage`, etc.)
*   Calculation of live permissions from stack definitions.
*   Compilation of stack definitions to CloudFormation/Azure ARM for initial setup.
*   Stack execution lifecycle: provision, refresh, upgrade, destroy.


## 3. API

```rust
// Define stacks programmatically
let stack = Stack::new().add(my_resource);

// Compile stacks to platform-specific templates
let template = stack.to_cloudformation()?;

// Fetch the current state of deployed resources
let state = State::fetch_from_cloudformation(&stack, "my-stack-name")?;

// Execute incremental deployment steps
let next_state = alien_infra::run_step(state).await?;
```

## 4. Flow - step by step

### 4.1 Initial Setup

This flow covers deploying the initial infrastructure, focusing on resources marked with `initialSetup = true`.

#### 4.1.1 Parse stack configuration

`alien-infra` uses Rust objects (`Storage`, `Function`, `Service`, etc.) to define resources, similar to Pulumi.

Example:

```rust
use alien_infra::{Stack, Function, Redis, Storage, StorageEvent, StorageTrigger};

// Define resources
let data_bucket = Storage::new("my-data");
let app_cache = Redis::new("my-cache");

// Define a function that processes data from the bucket and uses the cache
let processor_func = Function::new("process-data")
   .image("my-registry/process-data:latest")
   .links(vec![&app_cache, &data_bucket])
   .trigger(StorageTrigger::new(&data_bucket).on_event(StorageEvent::Create));

// Assemble the stack
let my_stack = Stack::new()
    .add(data_bucket)
    .add(app_cache)
    .add(processor_func);

// 'my_stack' now holds the definition.
```

Simplicity Constraint: You can only reference other resource *objects*, not their specific fields or outputs.

This is disallowed:

```rust
let processor_func = Function::new("process-data")
   .env("CACHE_URL", app_cache.output().host)  ❌
   // ...
```

Reason: Accessing outputs directly can complicate string formatting, type conversions, etc.

Consider using `bon` for automatically deriving the builder pattern.

#### 4.1.2 Expand to Platform-Specific Resources

`alien-infra` converts abstract definitions (`Storage`, `Function`, `Redis`) into concrete platform resources (AWS, GCP, Azure).

During expansion, `alien-infra` might also provision foundational resources (e.g., VPCs, IAM roles) based on stack needs.

Example AWS Resource Mapping:

| Name                     | Type                       | Origin                                                         |
| :----------------------- | :------------------------- | :------------------------------------------------------------- |
| `data-bucket`            | `AWSS3Bucket`              | From `Storage` (`data_bucket`)                                 |
| `app-cache`              | `AWSRedisInstance`         | From `Redis` (`app_cache`)                                     |
| `processor-func`         | `AWSLambdaFunction`        | From `Function` (`processor_func`)                             |
| `processor-func-url`     | `AWSLambdaFunctionUrl`     | From `Function` (`processor_func`, if configured)            |
| `processor-func-trigger` | `AWSLambdaBucketTrigger`   | From `Function` (`processor_func` trigger)                   |
| `app-vpc`                | `AWSVpc`                   | Auto-added (networking needs)                                  |
| `app-subnet`             | `AWSSubnet`                | Auto-added (networking needs)                                  |
| `cross-account-mgmt`     | `AWSIAMRole`               | Auto-added (management access, if configured)                |
| ...                      | ...                        | ...                                                            |

This step prepares the full set of cloud resources for template-based initial deployment.

#### 4.1.3 Generate and Deploy Initial Setup Template

For platforms like AWS/Azure requiring templates for initial setup, `alien-infra` compiles:
*   Resources marked with `initialSetup = true`
    (this includes all `frozen` resources and optionally some `live` ones).
*   Necessary foundational resources (like VPCs or IAM roles) automatically determined based on stack needs.
*   Required management permissions for subsequent live updates.

... into a platform-specific template (e.g., CloudFormation, ARM).

Example API (Rust):

```rust
// Assuming 'my_stack' is a Stack instance and 'aws_provider' is an AwsProvider instance.
// Example API for generating a CloudFormation template using the trait
let cfn_template = my_stack.to_cloudformation(&aws_provider)?;
```

Deployment (by user with elevated permissions):

```bash
# Example AWS CLI command for deploying the generated CloudFormation template
aws cloudformation deploy \
    --template-file initial_setup.yaml \
    --stack-name my-alien-app-initial-setup \
    --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM \
    --region us-east-1 \
    --parameter-overrides ManagerAccountId=123456789012 # Example parameter if needed
```

This deploys the foundation and grants `alien-infra` (via Managing Account/Function) permissions for live updates.

#### 4.1.4 Register Deployment and Fetch Initial State

After the initial infrastructure deployment (e.g., via CloudFormation on AWS, ARM on Azure), the deployment is "registered" with the Alien platform, making it known to the management system.

The Alien platform (or managing component) then uses `alien-infra` to fetch the current state of deployed resources. This involves platform-specific API calls to discover resource details. The timing depends on the platform: for template-based deployments like AWS/Azure, the fetch happens *after* template deployment. For dynamic flows like GCP's "Login with Google", `alien-infra` might deploy directly with temporary credentials, making the state known immediately post-deployment and potentially skipping a separate fetch step.

The method for populating the `State` object depends on the initial deployment approach. The `Stack` definition provided by the user is crucial here, as it's used to map the logical resources (like `my-data`) to the actual physical resources created in the cloud.

```rust
// The stack definition is needed to map the deployed physical resources
// back to the abstract resources defined by the user.
// For example, a single `Function` might result in a Lambda, a URL, and a trigger.

// CloudFormation:
// 1. Calls `DescribeStackResources` for initial resource names/IDs.
// 2. Calls details APIs (`DescribeBuckets`, `GetFunctionConfiguration`, etc.).
State::fetch_from_cloudformation(stack_definition: &Stack, stack_name: &str, ...)?;

// Azure ARM:
// 1. Calls Azure Resource Manager APIs for resource IDs/details.
State::fetch_from_arm(stack_definition: &Stack, deployment_name: &str, ...)?;

// Google Cloud (Direct Deployment):
// State might be constructed directly during deployment steps.
// May use an internal function like `State::construct_from_direct_deployment(...)`.

// Terraform State (Theoretical):
// Fetch state from a Terraform state file location.
State::fetch_from_terraform(stack_definition: &Stack, state_location: &str, ...)?;

// Manual Input (Theoretical):
// For non-template setups (e.g., AWS without CloudFormation).
// User provides JSON mapping logical resource names to physical IDs/details.
// Necessity TBD.
State::fetch_from_manual_input(stack_definition: &Stack, input_json: &str, ...)?;
```

As an example, the CloudFormation fetch process typically starts by calling `DescribeStackResources`:

```json
{ 
  "StackResources": [
    {
      "LogicalResourceId": "MyDataS3Bucket",
      "PhysicalResourceId": "my-alien-app-initial-setup-mydatas3bucket-12345ABCDEF",
      "ResourceType": "AWS::S3::Bucket"
    },
    {
      "LogicalResourceId": "ProcessDataLambdaFunction",
      "PhysicalResourceId": "arn:aws:lambda:us-east-1:111122223333:function:my-alien-app-proc-data",
      "ResourceType": "AWS::Lambda::Function"
    }
    // ... other resources ...
  ]
}
```

`alien-infra` then uses these initial identifiers and subsequent detailed API calls (like `GetFunctionConfiguration`, `DescribeBuckets`) to build the internal `State` object.

The resulting `State` object maps the logical resource definitions (e.g., `my-data`, `process-data`) to their corresponding physical cloud resources and their current configurations:

```json
{
  "resources": {
    "my-data": {
      "definition": {
        "type": "Storage",
        "name": "my-data", /*...*/
      },
      "status": "Exists",
      "outputs": {
        "bucketName": "..." /*, ...*/
      },
      "physicalResources": {
        "AWS::S3::Bucket": {
          "physicalId": "my-alien-app-initial-setup-mydatas3bucket-12345ABCDEF",
          "lastKnownConfig": { /* Detailed S3 Config fetched via AWS API */}
        }
      }
    },
    "process-data": {
      "definition": {
        "type": "Function",
        "name": "process-data", /*...*/
      },
      "status": "Exists",
      "outputs": {
        "url": "..." /*, ...*/
      },
      "physicalResources": {
        "AWS::Lambda::Function": {
          "physicalId": "arn:aws:lambda:us-east-1:111122223333:function:my-alien-app-proc-data",
          "lastKnownConfig": { /* Detailed Lambda Config fetched via AWS API */}
        },
        "AWS::Lambda::Url": { // Example for associated Function URL
          "physicalId": "https://abc123xyz.lambda-url.us-east-1.on.aws/",
          "lastKnownConfig": { /* Detailed Lambda URL Config */}
        },
        "AWS::Lambda::EventSourceMapping": { // Example for associated Trigger
          "physicalId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890", // UUID of the mapping
          "lastKnownConfig": { /* Detailed Trigger/Event Source Mapping Config */}
        }
        // Other related resources like IAM Role could also be listed here
      }
    }
    // ... other resources ...
  }
}
```

This fetched `State` is stored by Alien, becoming the baseline for the Live Deployment Flow.

### 4.2 Live Deployment Flow

This flow uses `alien-infra` to manage ongoing updates based on the stored `State`.

Typically used *after* Initial Setup (4.1) to deploy/update `live` resources.

Can also handle *initial* deployment (including `initialSetup=true` resources) if the initial setup template was bypassed (e.g., GCP "Login with Google" flow), starting from an empty state.

**Execute State Transition Step-by-Step**

The live deployment flow starts with the `run_step` API.
`run_step` executes one step of the deployment plan, including parallelizable operations based on the current state and dependency graph (see 4.2.2).

```rust
/// Executes one step of the deployment plan.
/// Identifies ready operations, initiates them, returns new state.
/// Designed for repeated calls by a durable execution framework.
///
/// Args:
///   current_state: Last known stack state.
///
/// Returns:
///   Ok(new_state): Updated state after operations (shows progress/completion).
///   Err(error):    If an error occurred.
async fn run_step(current_state: State) -> Result<State, Error>;
```

**Durable Workflow Design:**
*   `alien-infra`'s engine is stateful and designed for durable workflows.
*   It relies on an external orchestrator (e.g., Temporal, Inngest, Restate, custom state machine).
*   The orchestrator repeatedly calls `run_step`, persists state, handles scheduling, retries, and completion.

This step-by-step, stateful approach + external orchestrator enables reliable updates, ensuring progress despite transient failures or long operations.

#### 4.2.1 Calculate State Diff

Before building the dependency graph, `alien-infra` determines the operations needed to transition from `current_state` to the `desired_state` (from the user's `Stack` definition).

Process:

1.  **Load Desired State:** Parse the user's `Stack` definition.
2.  **Compare Resources:** Iterate through desired resources and compare with the current state:
    *   **New:** Resource in `desired_state` only -> `Create` operation.
    *   **Existing:** Resource in both states:
        *   Configs differ -> `Update` operation.
        *   Configs match -> `No-Op` (Read might still be needed for dependencies).
    *   **Removed:** Resource in `current_state` only -> `Delete` operation.
3.  **Output Diff:** Result is a list of planned operations (`Create`, `Update`, `Delete`).

This diff is the input for building the operation dependency graph.

#### 4.2.2 Build Operation Dependency Graph

Using the calculated diff (from 4.2.1), `alien-infra` builds a dependency graph of the required actions (Create, Update, Delete, Read).

*   Primarily involves operations on `live` resources.
*   Includes read operations on any resource whose outputs are needed by others.
*   Dependencies ensure correct execution order.

Example Nodes: `Create(Lambda)`, `Update(LambdaCode)`, `Delete(Redis)`, `Read(S3Bucket)`.
Example Edges: `Create(LambdaTrigger)` depends on `Create(Lambda)`.

```ascii
+-----------------+      +-------------------+
| Read(S3Bucket)  |----->| Create(IAMRole)   |  // Role needs bucket ARN
+-----------------+      +-------------------+
     |                         |
     +-------------------------+
               |
               v
         +----------------+
         | Create(Lambda) | // Operation to create Lambda if not exists
         +----------------+
               |
     +---------+---------+
     |                   |
     v                   v
+-------------------+  +--------------------------+
| Create(LambdaUrl) |  | Create(LambdaS3Trigger)  | // Operations dependent on Lambda
+-------------------+  +--------------------------+
```

Graph Validation:
*   Checked for cycles (e.g., using Tarjan's algorithm for SCCs).
*   Potentially simplified (e.g., using transitive reduction) before execution.


# 5. Core Components

This section defines the fundamental architecture of `alien-infra`, establishing a cloud-agnostic, extensible design with clear separation of concerns.

## 5.1 Resource Abstractions

The foundation of `alien-infra` is a set of core traits that define the behavior of resources and their interactions.

### 5.1.1 The `Resource` Trait

```rust
/// Core trait for all cloud resources
pub trait Resource: Send + Sync + Debug {
    /// Get unique identifier for this resource
    fn id(&self) -> &str;
    
    /// Check if this resource is considered "live" (can change frequently)
    fn is_live(&self) -> bool;
    
    /// Check if this resource should be created during initial setup
    fn initial_setup(&self) -> bool;
    
    /// Get all resources this resource depends on
    fn dependencies(&self) -> Vec<ResourceRef>;
    
    /// Get labels/tags to apply to this resource
    fn labels(&self) -> HashMap<String, String>;
}
```

The `ResourceRef` struct provides a safe way to reference resources:

```rust
/// A reference to another resource
#[derive(Clone, Debug)]
pub struct ResourceRef {
    resource_id: String,
}

impl ResourceRef {
    pub fn new<R: Resource>(resource: &R) -> Self {
        Self { resource_id: resource.id().to_string() }
    }
    
    pub fn id(&self) -> &str {
        &self.resource_id
    }
}
```

### 5.1.2 Cloud-Agnostic Resource Types

Here we define the concrete resource types implementing the `Resource` trait:

```rust
use bon::Builder;

#[derive(Builder, Debug)]
pub struct Function {
    id: String,
    #[builder(default = true)] // Default to live
    live: bool,
    #[builder(default = false)]
    initial_setup: bool,
    #[builder(default)]
    runtime: Option<String>,
    #[builder(default)]
    image: Option<String>,
    #[builder(default)]
    code: Option<CodeSource>,
    #[builder(default)]
    dependencies: Vec<ResourceRef>,
    #[builder(default)]
    env_vars: HashMap<String, String>,
    #[builder(default)]
    triggers: Vec<Box<dyn Trigger>>,
    #[builder(default)]
    labels: HashMap<String, String>,
}

impl Function {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            live: true, // Default to live
            initial_setup: false,
            runtime: None,
            image: None,
            code: None,
            dependencies: Vec::new(),
            env_vars: HashMap::new(),
            triggers: Vec::new(),
            labels: HashMap::new(),
        }
    }
    
    pub fn links(mut self, resources: Vec<&dyn Resource>) -> Self {
        for resource in resources {
            self.dependencies.push(ResourceRef::new(resource));
        }
        self
    }
    
    pub fn trigger(mut self, trigger: impl Trigger + 'static) -> Self {
        self.triggers.push(Box::new(trigger));
        self
    }
}

impl Resource for Function {
    fn id(&self) -> &str {
        &self.id
    }
    
    fn is_live(&self) -> bool {
        self.live
    }
    
    fn initial_setup(&self) -> bool {
        self.initial_setup
    }
    
    fn dependencies(&self) -> Vec<ResourceRef> {
        // Include explicit dependencies plus dependencies derived from triggers
        let mut deps = self.dependencies.clone();
        for trigger in &self.triggers {
            if let Some(dep) = trigger.source_resource() {
                deps.push(dep);
            }
        }
        deps
    }
    
    fn labels(&self) -> HashMap<String, String> {
        self.labels.clone()
    }
}

// Similar implementations for Storage, Queue, Database, etc.
#[derive(Builder, Debug)]
pub struct Storage {
    id: String,
    #[builder(default = false)] // Default to not live
    live: bool,
    #[builder(default = true)]
    initial_setup: bool,
    #[builder(default)]
    public: bool,
    #[builder(default)]
    versioning: bool,
    #[builder(default)]
    labels: HashMap<String, String>,
}

impl Storage {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            live: false, // Default to not live
            initial_setup: true,
            public: false,
            versioning: false,
            labels: HashMap::new(),
        }
    }
}

impl Resource for Storage {
    fn id(&self) -> &str {
        &self.id
    }
    
    fn is_live(&self) -> bool {
        self.live
    }
    
    fn initial_setup(&self) -> bool {
        self.initial_setup
    }
    
    fn dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }
    
    fn labels(&self) -> HashMap<String, String> {
        self.labels.clone()
    }
}
```

### 5.1.3 Triggers and Events

Trigger definitions connect resources to event sources:

```rust
pub trait Trigger: Debug {
    /// Get the type of this trigger
    fn trigger_type(&self) -> &'static str;
    
    /// Get the source resource this trigger is attached to (if any)
    fn source_resource(&self) -> Option<ResourceRef>;
    
    /// Convert to platform-specific configuration
    fn to_config(&self) -> Result<serde_json::Value, Error>;
}

#[derive(Debug)]
pub struct StorageTrigger {
    resource_ref: ResourceRef,
    events: Vec<StorageEvent>,
}

impl StorageTrigger {
    pub fn new(storage: &impl Resource) -> Self {
        Self {
            resource_ref: ResourceRef::new(storage),
            events: Vec::new(),
        }
    }
    
    pub fn on_event(mut self, event: StorageEvent) -> Self {
        self.events.push(event);
        self
    }
}

impl Trigger for StorageTrigger {
    fn trigger_type(&self) -> &'static str {
        "storage"
    }
    
    fn source_resource(&self) -> Option<ResourceRef> {
        Some(self.resource_ref.clone())
    }
    
    fn to_config(&self) -> Result<serde_json::Value, Error> {
        Ok(json!({
            "type": "storage",
            "resource": self.resource_ref.id(),
            "events": self.events.iter().map(|e| match e {
                StorageEvent::Create => "create",
                StorageEvent::Delete => "delete",
                StorageEvent::Update => "update",
            }).collect::<Vec<_>>()
        }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageEvent {
    Create,
    Delete,
    Update,
}
```

## 5.2 Platform Abstraction

The platform layer maps cloud-agnostic resources to platform-specific implementations through a collection of focused traits.

### 5.2.1 Platform-Specific Resources

```rust
/// Represents a platform-specific resource
#[derive(Debug, Clone)]
pub struct PlatformResource {
    /// Logical ID in the template
    logical_id: String,
    
    /// Resource type (e.g., "AWS::Lambda::Function")
    resource_type: String,
    
    /// Resource properties
    properties: serde_json::Value,
    
    /// Dependencies on other resources
    depends_on: Vec<String>,
    
    /// If this resource creates its own IAM permissions
    creates_iam_permissions: bool,
}

impl PlatformResource {
    pub fn new(
        logical_id: &str,
        resource_type: &str,
        properties: serde_json::Value,
    ) -> Self {
        Self {
            logical_id: logical_id.to_string(),
            resource_type: resource_type.to_string(),
            properties,
            depends_on: Vec::new(),
            creates_iam_permissions: false,
        }
    }
    
    pub fn add_dependency(&mut self, logical_id: &str) -> &mut Self {
        self.depends_on.push(logical_id.to_string());
        self
    }
    
    pub fn mark_creates_iam_permissions(&mut self) -> &mut Self {
        self.creates_iam_permissions = true;
        self
    }
}
```

### 5.2.2 Resource Expansion

```rust
/// Trait for expanding resources to platform-specific resources
pub trait ExpandResource {
    /// Expand a resource into platform-specific resources
    fn expand_resource(&self, resource: &dyn Resource) -> Result<Vec<PlatformResource>, Error>;
}

#[derive(Debug)]
pub struct AwsResourceExpander {
    region: String,
    account_id: Option<String>,
}

impl AwsResourceExpander {
    pub fn new(region: &str) -> Self {
        Self {
            region: region.to_string(),
            account_id: None,
        }
    }
    
    pub fn with_account_id(mut self, account_id: &str) -> Self {
        self.account_id = Some(account_id.to_string());
        self
    }
}

impl ExpandResource for AwsResourceExpander {
    fn expand_resource(&self, resource: &dyn Resource) -> Result<Vec<PlatformResource>, Error> {
        // Type-dispatch to specific expansion methods
        if let Some(function) = resource.as_any().downcast_ref::<Function>() {
            self.expand_function(function)
        } else if let Some(storage) = resource.as_any().downcast_ref::<Storage>() {
            self.expand_storage(storage)
        } else {
            // Fallback for unknown resources
            Err(Error::UnsupportedResource(resource.id().to_string()))
        }
    }
}

impl AwsResourceExpander {
    fn expand_function(&self, function: &Function) -> Result<Vec<PlatformResource>, Error> {
        let mut resources = Vec::new();
        
        // Create the Lambda function resource
        let mut lambda = PlatformResource::new(
            &format!("{}Lambda", function.id()),
            "AWS::Lambda::Function",
            json!({
                "FunctionName": function.id(),
                "Runtime": function.runtime.as_deref().unwrap_or("nodejs16.x"),
                "Role": { "Fn::GetAtt": [format!("{}Role", function.id()), "Arn"] },
                // Other properties...
            }),
        );
        
        // Create IAM role for the function
        let role = PlatformResource::new(
            &format!("{}Role", function.id()),
            "AWS::IAM::Role",
            json!({
                "AssumeRolePolicyDocument": {
                    "Version": "2012-10-17",
                    "Statement": [{
                        "Effect": "Allow",
                        "Principal": { "Service": "lambda.amazonaws.com" },
                        "Action": "sts:AssumeRole"
                    }]
                },
                // Other properties...
            }),
        );
        lambda.add_dependency(&role.logical_id);
        
        resources.push(lambda);
        resources.push(role);
        
        // Add function URL if needed
        let url = PlatformResource::new(
            &format!("{}Url", function.id()),
            "AWS::Lambda::Url",
            json!({
                "TargetFunctionArn": { "Fn::GetAtt": [format!("{}Lambda", function.id()), "Arn"] },
                "AuthType": "NONE",
            }),
        );
        resources.push(url);
        
        // Add triggers
        for trigger in &function.triggers {
            if let Some(trigger_resources) = self.expand_trigger(trigger, function)? {
                resources.extend(trigger_resources);
            }
        }
        
        Ok(resources)
    }
    
    fn expand_storage(&self, storage: &Storage) -> Result<Vec<PlatformResource>, Error> {
        let bucket = PlatformResource::new(
            &format!("{}S3Bucket", storage.id()),
            "AWS::S3::Bucket",
            json!({
                "BucketName": storage.id(),
                "VersioningConfiguration": {
                    "Status": if storage.versioning { "Enabled" } else { "Suspended" }
                },
                "PublicAccessBlockConfiguration": {
                    "BlockPublicAcls": !storage.public,
                    "BlockPublicPolicy": !storage.public,
                    "IgnorePublicAcls": !storage.public,
                    "RestrictPublicBuckets": !storage.public
                },
                // Other properties...
            }),
        );
        
        Ok(vec![bucket])
    }
    
    fn expand_trigger(
        &self,
        trigger: &Box<dyn Trigger>,
        function: &Function,
    ) -> Result<Option<Vec<PlatformResource>>, Error> {
        match trigger.trigger_type() {
            "storage" => {
                let storage_trigger = trigger.as_any().downcast_ref::<StorageTrigger>()
                    .ok_or_else(|| Error::InvalidTrigger("Not a StorageTrigger".into()))?;
                
                // Create S3 bucket notification configuration
                let trigger_resource = PlatformResource::new(
                    &format!("{}S3Trigger", function.id()),
                    "AWS::Lambda::Permission",
                    json!({
                        "Action": "lambda:InvokeFunction",
                        "FunctionName": { "Fn::GetAtt": [format!("{}Lambda", function.id()), "Arn"] },
                        "Principal": "s3.amazonaws.com",
                        // Other properties...
                    }),
                );
                
                Ok(Some(vec![trigger_resource]))
            }
            // Other trigger types...
            _ => Ok(None),
        }
    }
}
```

## 5.3 Permission Model

### 5.3.1 Permission Types

```rust
/// Represents a cloud platform permission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    /// Permission action (e.g., "lambda:UpdateFunctionCode")
    action: String,
    
    /// Resource this permission applies to (can use wildcards)
    resource: Option<String>,
    
    /// Additional conditions
    conditions: HashMap<String, serde_json::Value>,
}

impl Permission {
    pub fn new(action: &str) -> Self {
        Self {
            action: action.to_string(),
            resource: None,
            conditions: HashMap::new(),
        }
    }
    
    pub fn with_resource(mut self, resource: String) -> Self {
        self.resource = Some(resource);
        self
    }
    
    pub fn with_condition(mut self, key: &str, value: serde_json::Value) -> Self {
        self.conditions.insert(key.to_string(), value);
        self
    }
}

/// A group of related permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPolicy {
    /// Policy identifier
    id: String,
    
    /// Policy description
    description: Option<String>,
    
    /// Permissions in this policy
    permissions: Vec<Permission>,
}

impl PermissionPolicy {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            description: None,
            permissions: Vec::new(),
        }
    }
    
    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }
    
    pub fn with_permission(mut self, permission: Permission) -> Self {
        self.permissions.push(permission);
        self
    }
}
```

### 5.3.2 Live Permissions Calculator

```rust
/// Trait for calculating live permissions
pub trait CalculateLivePermissions {
    /// Calculate permissions needed for live management of a resource
    fn calculate_live_permissions(&self, resource: &dyn Resource) -> Result<Vec<Permission>, Error>;
}

pub struct AwsLivePermissionsCalculator {
    region: String,
    account_id: Option<String>,
}

impl AwsLivePermissionsCalculator {
    pub fn new(region: &str) -> Self {
        Self {
            region: region.to_string(),
            account_id: None,
        }
    }
    
    pub fn with_account_id(mut self, account_id: &str) -> Self {
        self.account_id = Some(account_id.to_string());
        self
    }
}

impl CalculateLivePermissions for AwsLivePermissionsCalculator {
    fn calculate_live_permissions(&self, resource: &dyn Resource) -> Result<Vec<Permission>, Error> {
        if let Some(function) = resource.as_any().downcast_ref::<Function>() {
            if !function.is_live() {
                // Non-live functions only need read permissions
                Ok(vec![
                    Permission::new("lambda:GetFunction")
                        .with_resource(format!("arn:aws:lambda:{}:{}:function:{}", 
                            self.region, self.account_id.as_deref().unwrap_or("*"), function.id())),
                    // Other read permissions...
                ])
            } else {
                // Live functions need update permissions
                Ok(vec![
                    Permission::new("lambda:GetFunction")
                        .with_resource(format!("arn:aws:lambda:{}:{}:function:{}", 
                            self.region, self.account_id.as_deref().unwrap_or("*"), function.id())),
                    Permission::new("lambda:UpdateFunctionCode")
                        .with_resource(format!("arn:aws:lambda:{}:{}:function:{}", 
                            self.region, self.account_id.as_deref().unwrap_or("*"), function.id())),
                    // Other update permissions...
                ])
            }
        } else if let Some(storage) = resource.as_any().downcast_ref::<Storage>() {
            // Usually only monitoring permissions for storage
            Ok(vec![
                Permission::new("s3:GetBucketLocation")
                    .with_resource(format!("arn:aws:s3:::{}", storage.id())),
                Permission::new("s3:ListBucket")
                    .with_resource(format!("arn:aws:s3:::{}", storage.id())),
                // Other permissions...
            ])
        } else {
            // Default permissions for unknown resources
            Ok(Vec::new())
        }
    }
}
```

## 5.4 State Management

The state management subsystem tracks deployed resources and their configurations.

### 5.4.1 Resource State

```rust
/// Represents the current state of a deployed resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceState {
    /// Resource identifier
    id: String,
    
    /// Current status of the resource
    status: ResourceStatus,
    
    /// Output values from the resource
    outputs: HashMap<String, serde_json::Value>,
    
    /// Platform-specific physical resources that implement this logical resource
    physical_resources: HashMap<String, PhysicalResourceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResourceStatus {
    NotExists,
    Creating,
    Exists,
    Updating,
    Deleting,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalResourceState {
    /// Physical ID of the resource
    physical_id: String,
    
    /// Last known configuration
    last_known_config: serde_json::Value,
}
```

### 5.4.2 Stack State

```rust
/// Represents the current state of a deployed stack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// Stack identifier
    stack_id: String,
    
    /// Platform identifier
    platform_id: String,
    
    /// Resource states by resource ID
    resources: HashMap<String, ResourceState>,
    
    /// Timestamp of last update
    last_updated: DateTime<Utc>,
}

impl State {
    pub fn new(stack_id: &str, platform_id: &str) -> Self {
        Self {
            stack_id: stack_id.to_string(),
            platform_id: platform_id.to_string(),
            resources: HashMap::new(),
            last_updated: Utc::now(),
        }
    }
    
    /// Update state with new resource status
    pub fn update_resource_status(
        &mut self, 
        resource_id: &str, 
        status: ResourceStatus,
    ) -> Result<(), Error> {
        if let Some(resource) = self.resources.get_mut(resource_id) {
            resource.status = status;
            self.last_updated = Utc::now();
            Ok(())
        } else {
            Err(Error::ResourceNotFound(resource_id.to_string()))
        }
    }
    
    /// Add output to a resource
    pub fn add_resource_output(
        &mut self,
        resource_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), Error> {
        if let Some(resource) = self.resources.get_mut(resource_id) {
            resource.outputs.insert(key.to_string(), value);
            self.last_updated = Utc::now();
            Ok(())
        } else {
            Err(Error::ResourceNotFound(resource_id.to_string()))
        }
    }
}
```

### 5.4.3 State Fetching

State fetching is handled by platform-specific static methods associated with the `State` struct. 

These methods interact directly with the cloud provider's APIs to determine the current status and configuration of deployed resources based on the initial deployment mechanism and the user's `Stack` definition.

Example methods (as introduced in Section 4.1.4):

```rust
impl State {
    /// Fetches state from AWS CloudFormation.
    /// Uses the stack name and definition to query AWS APIs 
    /// (e.g., DescribeStackResources, GetFunctionConfiguration) 
    /// and map physical resources back to the logical resources in the State.
    pub fn fetch_from_cloudformation(stack_definition: &Stack, stack_name: &str /*, other_params... */) -> Result<Self, Error> {
        // Implementation details...
        unimplemented!()
    }

    /// Fetches state from Azure Resource Manager (ARM).
    /// Uses the deployment name and definition to query Azure APIs.
    pub fn fetch_from_arm(stack_definition: &Stack, deployment_name: &str /*, other_params... */) -> Result<Self, Error> {
        // Implementation details...
        unimplemented!()
    }

    // Potentially other methods for different platforms or state sources (e.g., Terraform state)
}
```

These methods are crucial for initializing the `State` object after the initial setup (Section 4.1.4) and providing the baseline for the Live Deployment Flow (Section 4.2).


## 5.5 Template Generation

```rust
/// Trait for generating deployment templates
pub trait GenerateTemplate {
    /// Generate a template for initial setup
    fn generate_template(&self, stack: &Stack) -> Result<String, Error>;
}

pub struct CloudFormationGenerator {
    region: String,
    account_id: Option<String>,
    resource_expander: AwsResourceExpander,
    permissions_calculator: AwsLivePermissionsCalculator,
}

impl CloudFormationGenerator {
    pub fn new(region: &str) -> Self {
        Self {
            region: region.to_string(),
            account_id: None,
            resource_expander: AwsResourceExpander::new(region),
            permissions_calculator: AwsLivePermissionsCalculator::new(region),
        }
    }
    
    pub fn with_account_id(mut self, account_id: &str) -> Self {
        self.account_id = Some(account_id.to_string());
        self.resource_expander = self.resource_expander.with_account_id(account_id);
        self.permissions_calculator = self.permissions_calculator.with_account_id(account_id);
        self
    }
    
    fn create_management_resources(
        &self,
        stack: &Stack,
        policies: &[PermissionPolicy],
    ) -> Result<Vec<PlatformResource>, Error> {
        match &stack.management {
            StackManagement::Account => {
                // Create cross-account role with live permissions
                self.create_cross_account_role(policies)
            }
            StackManagement::Function(function_id) => {
                // Create IAM role for the managing function
                self.create_function_role(function_id, policies)
            }
            StackManagement::None => Ok(Vec::new()),
        }
    }
    
    fn create_cross_account_role(
        &self,
        policies: &[PermissionPolicy],
    ) -> Result<Vec<PlatformResource>, Error> {
        // Create IAM role with trust policy for the managing account
        let mut resources = Vec::new();
        
        let role = PlatformResource::new(
            "CrossAccountManagementRole",
            "AWS::IAM::Role",
            json!({
                "AssumeRolePolicyDocument": {
                    "Version": "2012-10-17",
                    "Statement": [{
                        "Effect": "Allow",
                        "Principal": { "AWS": format!("arn:aws:iam::{}:root", self.account_id.as_deref().unwrap()) },
                        "Action": "sts:AssumeRole"
                    }]
                }
            }),
        );
        resources.push(role);
        
        // Create policy for each permission policy
        for (i, policy) in policies.iter().enumerate() {
            let policy_resource = PlatformResource::new(
                &format!("LivePermissionsPolicy{}", i),
                "AWS::IAM::Policy",
                json!({
                    "PolicyName": policy.id,
                    "PolicyDocument": {
                        "Version": "2012-10-17",
                        "Statement": policy.permissions.iter().map(|p| {
                            let mut statement = json!({
                                "Effect": "Allow",
                                "Action": p.action
                            });
                            
                            if let Some(resource) = &p.resource {
                                statement["Resource"] = json!(resource);
                            }
                            
                            if !p.conditions.is_empty() {
                                statement["Condition"] = json!(p.conditions);
                            }
                            
                            statement
                        }).collect::<Vec<_>>()
                    },
                    "Roles": [{ "Ref": "CrossAccountManagementRole" }]
                }),
            );
            resources.push(policy_resource);
        }
        
        Ok(resources)
    }
    
    fn create_function_role(
        &self,
        function_id: &str,
        policies: &[PermissionPolicy],
    ) -> Result<Vec<PlatformResource>, Error> {
        // Create policy for each permission policy
        let mut resources = Vec::new();
        for (i, policy) in policies.iter().enumerate() {
            let policy_resource = PlatformResource::new(
                &format!("FunctionLivePermissionsPolicy{}", i),
                "AWS::IAM::Policy",
                json!({
                    "PolicyName": policy.id,
                    "PolicyDocument": {
                        "Version": "2012-10-17",
                        "Statement": policy.permissions.iter().map(|p| {
                            let mut statement = json!({
                                "Effect": "Allow",
                                "Action": p.action
                            });
                            
                            if let Some(resource) = &p.resource {
                                statement["Resource"] = json!(resource);
                            }
                            
                            if !p.conditions.is_empty() {
                                statement["Condition"] = json!(p.conditions);
                            }
                            
                            statement
                        }).collect::<Vec<_>>()
                    },
                    "Roles": [{ "Ref": format!("{}Role", function_id) }]
                }),
            );
            resources.push(policy_resource);
        }
        
        Ok(resources)
    }
}

impl GenerateTemplate for CloudFormationGenerator {
    fn generate_template(&self, stack: &Stack) -> Result<String, Error> {
        // 1. Filter resources for initial setup
        let initial_resources: Vec<&dyn Resource> = stack.resources()
            .filter(|r| r.initial_setup())
            .collect();
        
        // 2. Expand to platform resources
        let mut platform_resources = Vec::new();
        for resource in &initial_resources {
            let expanded = self.resource_expander.expand_resource(*resource)?;
            platform_resources.extend(expanded);
        }
        
        // 3. Calculate live permissions
        let mut live_permission_policies = Vec::new();
        for resource in stack.resources() {
            // Skip non-live resources with initialSetup=true (they don't need live permissions)
            if !resource.is_live() && resource.initial_setup() {
                continue;
            }
            
            // Calculate permissions for this resource
            let resource_permissions = self.permissions_calculator.calculate_live_permissions(resource)?;
            
            if !resource_permissions.is_empty() {
                let policy = PermissionPolicy::new(&format!("{}Policy", resource.id()))
                    .with_description(&format!("Live management permissions for {}", resource.id()));
                
                let policy = resource_permissions.into_iter()
                    .fold(policy, |policy, perm| policy.with_permission(perm));
                
                live_permission_policies.push(policy);
            }
        }
        
        // 4. Create IAM resources for live permissions
        let management_resources = self.create_management_resources(stack, &live_permission_policies)?;
        platform_resources.extend(management_resources);
        
        // 5. Generate CloudFormation template
        let template = json!({
            "AWSTemplateFormatVersion": "2010-09-09",
            "Description": format!("Initial setup for stack {}", stack.id()),
            "Resources": platform_resources.into_iter().map(|r| {
                (r.logical_id, json!({
                    "Type": r.resource_type,
                    "Properties": r.properties,
                    "DependsOn": r.depends_on
                }))
            }).collect::<HashMap<_, _>>(),
            "Outputs": {
                // Add outputs for resource IDs and other important values
                // ...
            }
        });
        
        Ok(serde_json::to_string_pretty(&template)?)
    }
}
```

## 5.6 Stack Definition

```rust
use bon::Builder;

#[derive(Debug)]
pub struct Stack {
    id: String,
    resources: HashMap<String, Box<dyn Resource>>,
    management: StackManagement,
}

#[derive(Debug, Clone)]
pub enum StackManagement {
    /// Managed by a cloud account (ID provided at runtime)
    Account,
    
    /// Managed by a function in the stack
    Function(String),
    
    /// No explicit management (default)
    None,
}

impl Stack {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            resources: HashMap::new(),
            management: StackManagement::None,
        }
    }
    
    pub fn add<R: Resource + 'static>(&mut self, resource: R) -> &mut Self {
        let id = resource.id().to_string();
        self.resources.insert(id, Box::new(resource));
        self
    }
    
    pub fn managed_by_account(&mut self) -> &mut Self {
        self.management = StackManagement::Account;
        self
    }
    
    pub fn managed_by_function(&mut self, function_id: &str) -> &mut Self {
        self.management = StackManagement::Function(function_id.to_string());
        self
    }
    
    pub fn resources(&self) -> impl Iterator<Item = &dyn Resource> {
        self.resources.values().map(|r| r.as_ref() as &dyn Resource)
    }
    
    pub fn id(&self) -> &str {
        &self.id
    }
}
```

## 5.7 Execution Engine

```rust
/// Operations for deployments
pub enum DeploymentOperation {
    Create(String), // Resource ID
    Update(String), // Resource ID
    Delete(String), // Resource ID
    Read(String),   // Resource ID
    Wait(Duration), // Time to wait
}

/// Trait for executing deployment operations
pub trait ExecuteOperation {
    /// Execute a deployment operation
    fn execute_operation(
        &self,
        operation: &DeploymentOperation,
        stack: &Stack,
        state: &State,
    ) -> Result<ResourceState, Error>;
}

/// Calculates the difference between current state and desired state
pub fn calculate_diff(
    current_state: &State,
    stack: &Stack,
) -> Result<Vec<DeploymentOperation>, Error> {
    // Implementation logic moves here from DefaultDiffCalculator
    let mut operations = Vec::new();
    // TODO: Implement actual diff logic comparing stack resources 
    //       with current_state.resources to generate Create, Update, Delete ops.
    Ok(operations) 
}

/// Builds a dependency graph from operations
pub fn build_dependency_graph(
    operations: Vec<DeploymentOperation>,
    stack: &Stack,
) -> Result<OperationGraph, Error> {
    // Implementation logic moves here from DefaultGraphBuilder
    let mut graph = OperationGraph::new();
    // TODO: Implement actual graph building logic based on resource dependencies
    //       in the stack and the calculated operations.
    for op in operations {
        graph.add_operation(op);
        // Add dependencies based on stack.resources().dependencies()
    }
    Ok(graph)
}

/// A graph of operations with dependencies
pub struct OperationGraph {
    operations: Vec<DeploymentOperation>,
    dependencies: HashMap<usize, Vec<usize>>,
    completed: HashSet<usize>,
}

impl OperationGraph {
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            dependencies: HashMap::new(),
            completed: HashSet::new(),
        }
    }
    
    pub fn add_operation(&mut self, operation: DeploymentOperation) -> usize {
        let index = self.operations.len();
        self.operations.push(operation);
        index
    }
    
    pub fn add_dependency(&mut self, dependent: usize, dependency: usize) {
        self.dependencies.entry(dependent)
            .or_insert_with(Vec::new)
            .push(dependency);
    }
    
    pub fn ready_operations(&self) -> Vec<&DeploymentOperation> {
        self.operations.iter()
            .enumerate()
            .filter(|(i, _)| !self.completed.contains(i))
            .filter(|(i, _)| {
                !self.dependencies.contains_key(i) || 
                self.dependencies[i].iter().all(|dep| self.completed.contains(dep))
            })
            .map(|(_, op)| op)
            .collect()
    }
    
    pub fn mark_completed(&mut self, operation_index: usize) {
        self.completed.insert(operation_index);
    }
    
    pub fn is_completed(&self) -> bool {
        self.completed.len() == self.operations.len()
    }
}

/// Execute a single step of the deployment
pub async fn run_step(
    current_state: State,
    stack: &Stack,
    executor: &dyn ExecuteOperation,
) -> Result<State, Error> {
    // 1. Calculate difference using the standalone function
    let diff = calculate_diff(&current_state, stack)?;
    
    // 2. Build dependency graph using the standalone function
    let mut graph = build_dependency_graph(diff, stack)?;
    
    // 3. Find operations ready to execute (no pending dependencies)
    let ready_ops = graph.ready_operations();
    
    // 4. If no operations are ready, deployment is complete
    if ready_ops.is_empty() {
        return Ok(current_state);
    }
    
    // 5. Execute one operation
    let op = ready_ops[0];
    let mut new_state = current_state.clone();
    
    match executor.execute_operation(&op, stack, &current_state) {
        Ok(resource_state) => {
            // Update the state with the new resource state
            new_state.resources.insert(resource_state.id.clone(), resource_state);
        }
        Err(e) => {
            // Mark the resource as failed
            if let DeploymentOperation::Create(id) | DeploymentOperation::Update(id) | DeploymentOperation::Delete(id) = op {
                if let Some(resource) = new_state.resources.get_mut(id) {
                    resource.status = ResourceStatus::Failed(e.to_string());
                }
            }
            return Err(e);
        }
    }
    
    Ok(new_state)
}
```

## 5.8 Usage Example

Here's how a developer would use the library to define and deploy infrastructure:

```rust
use alien_infra::{Function, Storage, StorageTrigger, StorageEvent, Stack, Error};
use alien_infra::platforms::aws::{ 
    AwsResourceExpander,
    AwsLivePermissionsCalculator,
    CloudFormationGenerator,
    AwsOperationExecutor,
}; 
use alien_infra::deployment::run_step;
use alien_infra::state::State;

async fn deploy_example() -> Result<(), Error> {
    // Define resources
    let data_bucket = Storage::builder()
        .id("my-data".to_string())
        .live(false) // This bucket rarely changes (not live)
        .initial_setup(true)  // Create during initial setup
        .build();

    // Define a function triggered by file uploads
    let processor_fn = Function::builder()
        .id("data-processor".to_string())
        .image("my-registry/processor:latest".to_string())
        .build()
        // Link to other resources
        .links(vec![&data_bucket])
        // Add a trigger
        .trigger(StorageTrigger::new(&data_bucket).on_event(StorageEvent::Create));

    // Define the stack
    let mut stack = Stack::new("my-app");
    stack.add(data_bucket)
         .add(processor_fn)
         .managed_by_account();

    // For initial setup: generate CloudFormation template
    // The managing account ID is provided to the generator, not the stack definition
    let cfn_template = CloudFormationGenerator::new("us-east-1")
        .with_account_id("123456789012")
        .generate_template(&stack)?;

    // Save template to file
    std::fs::write("initial_setup.yaml", cfn_template)?;
    println!("Initial setup template generated! Deploy with AWS CloudFormation.");

    // After initial setup is deployed, fetch the initial state using the platform-specific method
    let stack_name = "my-app";
    let initial_state = State::fetch_from_cloudformation(&stack, stack_name /*, other_params... */)?;

    // For ongoing live deployments, execute steps
    let executor = AwsOperationExecutor::new("us-east-1");
    
    let new_state = run_step(
        initial_state,
        &stack,
        &executor
    ).await?;
    
    println!("Deployment step completed!");
    
    Ok(())
}

```

# 6. Implementation Milestones

This section outlines a phased approach to implementing `alien-infra`, focusing on delivering testable components incrementally.

## Milestone 1: Core Resource Abstractions

*   **Goal:** Define the fundamental building blocks for resources.
*   **Implementation:**
    *   Implement the `Resource` trait (`id`, `is_live`, `initial_setup`, `dependencies`, `labels`).
    *   Implement `ResourceRef` struct and its constructor.
    *   Implement basic cloud-agnostic resource structs (`Storage`, `Function`) using `bon::Builder`. Include basic fields like `id`, `live`, `initial_setup`, `labels`.
    *   Implement the `links` method for `Function`.
    *   Implement the base `Trigger` trait.
    *   Implement `StorageEvent` enum and `StorageTrigger` struct, implementing `Trigger`. Include `new` and `on_event` methods.
*   **Testing:**
    *   Unit tests for creating `Storage` and `Function` instances via builders.
    *   Unit tests verifying `id()`, `is_live()`, `initial_setup()`, `labels()` methods on resources.
    *   Unit tests for `ResourceRef::new()` and `id()`.
    *   Unit tests for `Function::links()`, verifying `dependencies()` includes linked resources.
    *   Unit tests for `StorageTrigger::new()` and `on_event()`, verifying `trigger_type()` and `source_resource()`.

## Milestone 2: Platform Abstraction Basics (AWS)

*   **Goal:** Define how platform-specific resources are represented and how abstract resources are expanded for AWS.
*   **Implementation:**
    *   Implement `PlatformResource` struct (`new`, `add_dependency`, `mark_creates_iam_permissions`).
    *   Implement the `ExpandResource` trait.
    *   Implement `AwsResourceExpander` struct (`new`, `with_account_id`).
    *   Implement `AwsResourceExpander::expand_resource` method, initially dispatching only to `expand_storage`.
    *   Implement `AwsResourceExpander::expand_storage` to convert a `Storage` resource into an `AWS::S3::Bucket` `PlatformResource`.
*   **Testing:**
    *   Unit tests for `PlatformResource` creation and methods.
    *   Unit tests for `AwsResourceExpander::expand_storage` ensuring correct logical ID, resource type (`AWS::S3::Bucket`), and basic properties (BucketName, VersioningConfiguration, PublicAccessBlockConfiguration) based on the input `Storage` resource.

## Milestone 3: Basic Template Generation (AWS CloudFormation)

*   **Goal:** Generate a minimal CloudFormation template for a simple stack.
*   **Implementation:**
    *   Implement the `GenerateTemplate` trait.
    *   Implement `CloudFormationGenerator` struct (`new`, `with_account_id`). Include `AwsResourceExpander` as a member.
    *   Implement `CloudFormationGenerator::generate_template`.
        *   Filter resources for `initial_setup` (using `stack.resources()`).
        *   Call `resource_expander.expand_resource` for each filtered resource (only `Storage` for now).
        *   Assemble the basic CloudFormation JSON structure (`AWSTemplateFormatVersion`, `Description`, `Resources`) using the generated `PlatformResource`s. Omit `Outputs`, permissions, and management logic for now.
    *   Implement `Stack` struct (`new`, `add`, `resources`, `id`). Omit `management` field for now.
*   **Testing:**
    *   Unit tests for `Stack::add`, `resources`, `id`.
    *   Integration test:
        *   Create a `Stack` with one `Storage` resource (`initial_setup=true`).
        *   Instantiate `CloudFormationGenerator`.
        *   Call `generate_template`.
        *   Parse the output string as JSON and verify:
            *   `AWSTemplateFormatVersion` is correct.
            *   `Resources` contains one entry with the correct logical ID.
            *   The resource entry has `Type: AWS::S3::Bucket` and correct `Properties` based on the `Storage` resource.

# TODO:

- What available resources do we have and what are their fields?
- Compilation from source code -> docker images (and adding alien-runtime)
    - How to configure the management access of the stack (cross-account role etc)
- Traits & main component flow
