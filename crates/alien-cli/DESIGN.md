# Alien CLI Specification

The Alien CLI is a command-line tool for initializing, developing, building, releasing, and deploying agent projects. It supports both local and SaaS operations, with commands designed to run seamlessly in either context where appropriate. The default behavior for certain commands is SaaS-based, but local alternatives are provided via flags or specific commands.


## Core Commands

These commands run locally but are relevant in both local and SaaS deployment scenarios.

### `alien init [template]`

- **Description**: Initializes a new project in the current directory using an optional template.
- **Behavior**: Creates a project structure locally. No SaaS interaction required.
- **Example**:

  ```bash
  alien init my-agent
  ```

### `alien dev`

- **Description**: Starts a local development server for working on the agent.
- **Behavior**: Runs locally without network or SaaS connectivity.
- **Example**:

  ```bash
  alien dev
  ```

### `alien run`

- **Description**: Runs the agent locally from the current build.
- **Behavior**: Executes the most recent local build. No SaaS required.
- **Example**:

  ```bash
  alien run
  ```

### `alien export <template> [options]`

- **Description**: Exports a deployment template for a specified platform.
- **Behavior**: Generates a template (e.g., CloudFormation for AWS) from the local project. No SaaS required by default.
- **Supported Templates**:
  - `alien --platform <platform>`: Exports an Alien template (serialized stack JSON).
  - `cloudformation [--platform aws]`: Exports an AWS CloudFormation template.
- **Options**:
  - `--local`: Use local build from .alien directory (required currently, SaaS support planned).
  - `--output <file>`: Output file path (optional, defaults to stdout).
  - `--all-resources`: (CloudFormation only) Include all resources, not just initial setup resources.
  - `--default-managing-account-id <id>`: (CloudFormation only) Default managing account ID.
- **Examples**:

  ```bash
  # Export Alien template for AWS
  alien export alien --local --platform aws

  # Export CloudFormation template
  alien export cloudformation --local

  # Export CloudFormation with all resources to file
  alien export cloudformation --local --all-resources --output template.yaml
  ```

---

## SaaS Commands

These commands interact directly with the Alien SaaS platform.

### `alien release`

- **Description**: Uploads the built agent as a new release to the SaaS.
- **Behavior**:
  - Requires a prior build (remote or local).
  - Uploads the build to the SaaS, which updates registered agents and provides an installation link.
  - Prompts login if not authenticated.
- **Example**:

  ```bash
  alien release
  ```

### `alien agents new [options]`

- **Description**: Registers a new agent with the SaaS.
- **Behavior**:
  - Registers the agent with the SaaS using provided options (e.g., `--target`, `--role-arn`).
  - SaaS handles deployment and management.
- **Options**:
  - `--target <platform>`: Target platform (e.g., `aws`). Required.
  - `--role-arn <arn>`: AWS management role ARN (for `aws` platform).
- **Example**:

  ```bash
  alien agents new --target aws --role-arn arn:aws:iam::123456789012:role/MyRole
  ```

### `alien agents ls`

- **Description**: Lists all agents registered with the SaaS.
- **Behavior**: Queries the SaaS for the list of agents.
- **Example**:

  ```bash
  alien agents ls
  ```

### `alien agents delete <agent-id>`

- **Description**: Deletes the specified agent from the SaaS.
- **Behavior**: Removes the agent from SaaS management.
- **Example**:

  ```bash
  alien agents delete agent-123
  ```

---

## Local Only Commands

These commands are exclusively for local operations and do not interact with the SaaS.

### `alien build`

- **Description**: Builds the agent project, building container images and generating platform-specific templates.
- **Behavior**: Builds locally and generates platform-specific deployment artifacts.
- **Options**:
  - `--output-dir <dir>`: Output directory (defaults to `.alien`).
  - `--platform <platform>`: Target platform (defaults to `aws`).
  - `--aws-managing-account-id <id>`: AWS managing account ID.
  - `--runtime-url <url>`: Alien runtime base URL.
  - `--image-repo <url>`: Image repository URL for pushing container images.
  - `--registry-auth <type>`: Registry auth type (`anonymous` or `basic`).
  - `--registry-protocol <protocol>`: Registry protocol (`http` or `https`).
  - `--registry-username <username>`: Registry username (for basic auth).
  - `--registry-password <password>`: Registry password (for basic auth).
- **Examples**:

  ```bash
  # Build for AWS with default settings
  alien build

  # Build for GCP with custom output directory
  alien build --platform gcp --output-dir ./build

  # Build and push to custom registry
  alien build --image-repo my-registry.com/my-app --registry-auth basic --registry-username user --registry-password pass
  ```

### `alien push --to <path>`

- **Description**: Pushes the locally built agent to a storage location (e.g., S3).
- **Behavior**:
  - Requires a prior local build (`alien build`).
  - Uploads the build to the specified path (e.g., S3 URL).
  - Outputs a run command like: "Run this: `alien run from s3://my-bucket/stack.json`".
- **Options**:
  - `--to <path>`: Destination path (required).
- **Example**:

  ```bash
  alien push --to s3://my-bucket/stack.json
  ```

### `alien run from <path>`

- **Description**: Pulls an agent from a remote location (e.g., S3) and runs it locally.
- **Behavior**: Downloads and executes the agent from the specified path. No SaaS required.
- **Example**:

  ```bash
  alien run from s3://my-bucket/stack.json
  ```

### `alien apply [options]`

- **Description**: Deploys the locally built agent to a target environment.
- **Behavior**:
  - Requires a prior local build (`alien build`).
  - Deploys to the specified target using deployment target options.
  - Stores state locally (e.g., `.alien/state.json`) with impersonation info for destroy operations.
- **Deployment Target Options** (choose one per platform):
  - `--target aws --current-account`: Deploy to current AWS account using existing credentials
  - `--target aws --role-arn <arn>`: Deploy to AWS via CloudFormation-managed role ARN
  - `--target gcp --current-account`: Deploy to current GCP account using existing credentials  
  - `--target gcp --sa <email>`: Deploy to GCP via service account impersonation
  - `--target azure --current-account`: Deploy to current Azure account using existing credentials
  - `--target azure --client-id <id>`: Deploy to Azure via managed identity impersonation
  - `--target local`: Deploy locally
  - `--target kubernetes`: Deploy to Kubernetes
- **Additional Options**:
  - `--external-id <id>`: External ID for AWS role assumption
  - `--duration-seconds <seconds>`: Session duration for AWS role assumption
  - `--scopes <scopes>`: GCP OAuth 2.0 scopes (comma-separated)
  - `--scope <scope>`: Azure scope for access token
  - `--tenant-id <id>`: Azure tenant ID for cross-tenant access
  - `--context <context>`: Kubernetes context to use
  - `--state-uri <uri>`: URI for storing stack state (defaults to `.alien/state.json`)
  - `--stack-file <file>`: Path to stack definition file (defaults to `.alien/stack.json`)
  - `--max-steps <count>`: Maximum number of reconciliation steps
- **Examples**:

  ```bash
  # Deploy to current AWS account
  alien apply --target aws --current-account
  
  # Deploy to AWS via management role
  alien apply --target aws --role-arn arn:aws:iam::123456789012:role/MyRole --external-id test123
  
  # Deploy to GCP via service account
  alien apply --target gcp --sa my-sa@my-project.iam.gserviceaccount.com
  
  # Deploy to Azure via managed identity
  alien apply --target azure --client-id 12345678-1234-1234-1234-123456789012
  
  # Deploy locally
  alien apply --target local
  ```

### `alien destroy`

- **Description**: Destroys the locally deployed resources.
- **Behavior**:
  - Uses the local state to identify and remove resources.
  - Automatically uses the same impersonation method that was used during deployment.
  - No SaaS interaction required.
- **Options**:
  - `--target <platform>`: Target platform (same options as apply)
  - `--state-uri <uri>`: URI for the stack state (defaults to `.alien/state.json`)
  - `--stack-file <file>`: Path to stack definition file (defaults to `.alien/stack.json`)
  - `--max-steps <count>`: Maximum number of reconciliation steps
- **Example**:

  ```bash
  # Destroy using same target as deployment
  alien destroy --target aws --role-arn arn:aws:iam::123456789012:role/MyRole
  ```

### `alien state-import [options]`

- **Description**: Imports state from existing infrastructure without deploying.
- **Behavior**:
  - Imports CloudFormation stack state into local state file.
  - Useful for taking over management of existing infrastructure.
  - Currently only supports AWS management role targets.
- **Options**:
  - `--target aws --role-arn <arn>`: AWS management role ARN to assume for import
  - `--external-id <id>`: External ID for AWS role assumption
  - `--duration-seconds <seconds>`: Session duration for AWS role assumption
  - `--output <file>`: Output file path for imported state (defaults to `.alien/<target>/state.json`)
  - `--stack-file <file>`: Path to stack definition file (defaults to `.alien/<target>/stack.json`)
- **Examples**:

  ```bash
  alien state-import --target aws --role-arn arn:aws:iam::123456789012:role/MyRole
  ```

---

## User Flows

### 1. Customer Deployment SaaS Workflow

This flow is for releasing an agent to the SaaS and providing an installation link to customers.

1. Initialize a project:

   ```bash
   alien init my-agent
   ```
2. Develop the agent:

   ```bash
   alien dev
   ```
3. Release the agent:

   ```bash
   alien release
   ```
4. Share the installation link with customers for SaaS-managed deployment.

### 2. Developer Testing SaaS Workflow

This flow is for testing an agent by registering it with the SaaS.

1. Initialize a project:

   ```bash
   alien init my-agent
   ```
2. Develop the agent:

   ```bash
   alien dev
   ```
3. Build the agent remotely:

   ```bash
   alien release
   ```
4. Export a CloudFormation template:

   ```bash
   alien export cloudformation --local
   ```
5. Install the template in AWS to get a role ARN.
6. Register a new agent with SaaS:

   ```bash
   alien agents new --target aws --role-arn arn:aws:iam::123456789012:role/MyRole
   ```

### 3. Robotics Workflow (Local)

This flow is for deploying an agent to a robot using local operations.

1. Initialize a project:

   ```bash
   alien init my-agent
   ```
2. Develop the agent:

   ```bash
   alien dev
   ```
3. Build the agent locally:

   ```bash
   alien build
   ```
4. Push the build to S3:

   ```bash
   alien push --to s3://my-bucket/stack.json
   ```
5. On the robot, run the agent:

   ```bash
   alien run from s3://my-bucket/stack.json
   ```

### 4. Local Cloud Deployment (Current Account)

This flow is for deploying an agent to a cloud platform using current account credentials.

1. Initialize a project:

   ```bash
   alien init my-agent
   ```
2. Develop the agent:

   ```bash
   alien dev
   ```
3. Build the agent locally:

   ```bash
   alien build
   ```
4. Deploy to current AWS account:

   ```bash
   alien apply --target aws --current-account
   ```
5. Destroy the deployment:

   ```bash
   alien destroy --target aws --current-account
   ```

### 5. Cross-Account AWS Deployment

This flow is for deploying an agent to AWS using a management role (typical enterprise scenario).

1. Initialize a project:

   ```bash
   alien init my-agent
   ```
2. Develop the agent:

   ```bash
   alien dev
   ```
3. Build the agent locally:

   ```bash
   alien build
   ```
4. Export a CloudFormation template:

   ```bash
   alien export cloudformation --local
   ```
5. Install the template in target AWS account to create management role.
6. Import existing state and deploy via management role:

   ```bash
   alien state-import --target aws --role-arn arn:aws:iam::123456789012:role/MyRole
   alien apply --target aws --role-arn arn:aws:iam::123456789012:role/MyRole --state-uri file://.alien/aws/state.json
   ```
7. Destroy the deployment (uses saved impersonation automatically):

   ```bash
   alien destroy --target aws --role-arn arn:aws:iam::123456789012:role/MyRole
   ```

### 6. Multi-Cloud Deployment

This flow demonstrates deploying to different cloud providers.

1. **GCP Deployment**:

   ```bash
   alien apply --target gcp --sa deployment-sa@my-project.iam.gserviceaccount.com
   ```

2. **Azure Deployment**:

   ```bash
   alien apply --target azure --client-id 12345678-1234-1234-1234-123456789012 --tenant-id 87654321-4321-4321-4321-210987654321
   ```

All cloud deployments automatically save impersonation information for consistent destroy operations.
