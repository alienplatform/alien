# openapi

Developer-friendly & type-safe Typescript SDK specifically catered to leverage *openapi* API.

<div align="left">
    <a href="https://www.speakeasy.com/?utm_source=openapi&utm_campaign=typescript"><img src="https://www.speakeasy.com/assets/badges/built-by-speakeasy.svg" /></a>
    <a href="https://opensource.org/licenses/MIT">
        <img src="https://img.shields.io/badge/License-MIT-blue.svg" style="width: 100px; height: 28px;" />
    </a>
</div>


<br /><br />
> [!IMPORTANT]
> This SDK is not yet ready for production use. To complete setup please follow the steps outlined in your [workspace](https://app.speakeasy.com/org/alien/alien). Delete this section before > publishing to a package manager.

<!-- Start Summary [summary] -->
## Summary


<!-- End Summary [summary] -->

<!-- Start Table of Contents [toc] -->
## Table of Contents
<!-- $toc-max-depth=2 -->
* [openapi](#openapi)
  * [SDK Installation](#sdk-installation)
  * [Requirements](#requirements)
  * [SDK Example Usage](#sdk-example-usage)
  * [Authentication](#authentication)
  * [Available Resources and Operations](#available-resources-and-operations)
  * [Standalone functions](#standalone-functions)
  * [Retries](#retries)
  * [Error Handling](#error-handling)
  * [Server Selection](#server-selection)
  * [Custom HTTP Client](#custom-http-client)
  * [Debugging](#debugging)
* [Development](#development)
  * [Maturity](#maturity)
  * [Contributions](#contributions)

<!-- End Table of Contents [toc] -->

<!-- Start SDK Installation [installation] -->
## SDK Installation

> [!TIP]
> To finish publishing your SDK to npm and others you must [run your first generation action](https://www.speakeasy.com/docs/github-setup#step-by-step-guide).


The SDK can be installed with either [npm](https://www.npmjs.com/), [pnpm](https://pnpm.io/), [bun](https://bun.sh/) or [yarn](https://classic.yarnpkg.com/en/) package managers.

### NPM

```bash
npm add <UNSET>
```

### PNPM

```bash
pnpm add <UNSET>
```

### Bun

```bash
bun add <UNSET>
```

### Yarn

```bash
yarn add <UNSET>
```

> [!NOTE]
> This package is published as an ES Module (ESM) only. For applications using
> CommonJS, use `await import()` to import and use this package.
<!-- End SDK Installation [installation] -->

<!-- Start Requirements [requirements] -->
## Requirements

For supported JavaScript runtimes, please consult [RUNTIMES.md](RUNTIMES.md).
<!-- End Requirements [requirements] -->

<!-- Start SDK Example Usage [usage] -->
## SDK Example Usage

### Example

```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.user.updateProfile();

  console.log(result);
}

run();

```
<!-- End SDK Example Usage [usage] -->

<!-- Start Authentication [security] -->
## Authentication

### Per-Client Security Schemes

This SDK supports the following security scheme globally:

| Name     | Type | Scheme      | Environment Variable |
| -------- | ---- | ----------- | -------------------- |
| `apiKey` | http | HTTP Bearer | `ALIEN_API_KEY`      |

To authenticate with the API the `apiKey` parameter must be set when initializing the SDK client instance. For example:
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.user.updateProfile();

  console.log(result);
}

run();

```
<!-- End Authentication [security] -->

<!-- Start Available Resources and Operations [operations] -->
## Available Resources and Operations

<details open>
<summary>Available methods</summary>

### [ApiKeys](docs/sdks/apikeys/README.md)

* [list](docs/sdks/apikeys/README.md#list) - Retrieve all API keys for the current workspace.
* [create](docs/sdks/apikeys/README.md#create) - Create a new API key.
* [get](docs/sdks/apikeys/README.md#get) - Retrieve a specific API key.
* [revoke](docs/sdks/apikeys/README.md#revoke) - Revoke (soft delete) an API key.
* [update](docs/sdks/apikeys/README.md#update) - Update an API key (enable/disable, change description).
* [deleteMultiple](docs/sdks/apikeys/README.md#deletemultiple) - Permanently delete multiple API keys.

### [Auth](docs/sdks/auth/README.md)

* [whoami](docs/sdks/auth/README.md#whoami) - Get current authenticated principal information (user or service account). Works with both session cookies and API keys.

### [Commands](docs/sdks/commands/README.md)

* [list](docs/sdks/commands/README.md#list) - Retrieve commands. Use for dashboard analytics and command history.
* [create](docs/sdks/commands/README.md#create) - Create command metadata. Called by manager when processing commands. Returns project info for routing decisions.
* [listNames](docs/sdks/commands/README.md#listnames) - List distinct command names. Use for filter dropdowns in the dashboard.
* [listDeployments](docs/sdks/commands/README.md#listdeployments) - List distinct deployments that have commands, including deployment group info. Use for filter dropdowns in the dashboard.
* [get](docs/sdks/commands/README.md#get) - Retrieve a command by ID.
* [update](docs/sdks/commands/README.md#update) - Update command state. Called by manager when command is dispatched or completes.

### [Containers](docs/sdks/containers/README.md)

* [getOverview](docs/sdks/containers/README.md#getoverview) - Bird's-eye view of all container definitions across all deployments, with aggregate health stats, machine health breakdown, and HTTP performance metrics.
* [getAttention](docs/sdks/containers/README.md#getattention) - Returns deployments that need attention: crash loops, scheduling failures, unhealthy machines.
* [getDeployments](docs/sdks/containers/README.md#getdeployments) - Per-deployment breakdown for a container: status, replicas, metrics, and HTTP performance across all deployments running this container.
* [getMachines](docs/sdks/containers/README.md#getmachines) - Cross-deployment machine health: per-deployment machine counts by status, capacity group utilization, and scaling recommendations.
* [getDeploymentCluster](docs/sdks/containers/README.md#getdeploymentcluster) - Container cluster overview for a specific deployment: machine count, container count, and capacity.
* [listDeploymentContainers](docs/sdks/containers/README.md#listdeploymentcontainers) - List all containers running in a specific deployment.
* [getDeploymentContainer](docs/sdks/containers/README.md#getdeploymentcontainer) - Get detailed status, configuration, and replica metrics for a specific container in a deployment.
* [listDeploymentMachines](docs/sdks/containers/README.md#listdeploymentmachines) - List all compute machines in a deployment's container cluster.
* [listDeploymentEvents](docs/sdks/containers/README.md#listdeploymentevents) - List orchestration events for a deployment's container cluster.
* [listDeploymentContainerInstanceEvents](docs/sdks/containers/README.md#listdeploymentcontainerinstanceevents) - List orchestration events for a specific container in a deployment.

### [Deployment](docs/sdks/deployment/README.md)

* [getInfo](docs/sdks/deployment/README.md#getinfo) - Get deployment information for the deployment page. Accepts both agent-scoped and deployment-group-scoped API keys. Returns project information, package status/outputs, and either agent or deployment group details depending on the token type. Poll this endpoint to check if packages are ready.

### [DeploymentGroups](docs/sdks/deploymentgroups/README.md)

* [listDeploymentGroups](docs/sdks/deploymentgroups/README.md#listdeploymentgroups) - List deployment groups
* [createDeploymentGroup](docs/sdks/deploymentgroups/README.md#createdeploymentgroup) - Create a new deployment group
* [getDeploymentGroup](docs/sdks/deploymentgroups/README.md#getdeploymentgroup) - Get deployment group details
* [deleteDeploymentGroup](docs/sdks/deploymentgroups/README.md#deletedeploymentgroup) - Delete deployment group
* [updateDeploymentGroup](docs/sdks/deploymentgroups/README.md#updatedeploymentgroup) - Update deployment group
* [createDeploymentGroupToken](docs/sdks/deploymentgroups/README.md#createdeploymentgrouptoken) - Create deployment group token

### [Deployments](docs/sdks/deployments/README.md)

* [list](docs/sdks/deployments/README.md#list) - Retrieve all deployments.
* [create](docs/sdks/deployments/README.md#create) - Create a new agent. Deployment group tokens automatically use their group. Workspace/project tokens must provide deploymentGroupId.
* [getStats](docs/sdks/deployments/README.md#getstats) - Get aggregated deployment statistics. Returns total count and breakdown by status.
* [listFilterPlatforms](docs/sdks/deployments/README.md#listfilterplatforms) - List distinct platforms used by deployments. Used for filter dropdowns.
* [listFilterDeploymentGroups](docs/sdks/deployments/README.md#listfilterdeploymentgroups) - List deployment groups with agent counts. Used for filter dropdowns.
* [get](docs/sdks/deployments/README.md#get) - Retrieve an agent by ID.
* [delete](docs/sdks/deployments/README.md#delete) - Delete an agent by ID. This can be used to start deletion or retry failed deletions.
* [getInfo](docs/sdks/deployments/README.md#getinfo) - Get deployment connection information including command endpoint and resource URLs.
* [import](docs/sdks/deployments/README.md#import) - Import an agent from existing infrastructure (e.g., CloudFormation stack). The agent ID is automatically generated.
* [redeploy](docs/sdks/deployments/README.md#redeploy) - Redeploy a running agent with the same release and fresh environment variables. Sets status to update-pending.
* [pinRelease](docs/sdks/deployments/README.md#pinrelease) - Pin or unpin deployment to a specific release. Only works for running deployments. Controller will automatically trigger update to target release.
* [retry](docs/sdks/deployments/README.md#retry) - Retry a failed agent operation. Uses alien-infra's retry mechanisms to resume from exact failure point.
* [updateEnvironmentVariables](docs/sdks/deployments/README.md#updateenvironmentvariables) - Update an agent's environment variables. If the agent is running and not locked, the status will be changed to update-pending to trigger a deployment.
* [createToken](docs/sdks/deployments/README.md#createtoken) - Create an agent token (agent-scoped API key) for this agent. The agent must exist before creating a token.

### [Domains](docs/sdks/domains/README.md)

* [list](docs/sdks/domains/README.md#list) - List system domains and workspace domains.
* [create](docs/sdks/domains/README.md#create) - Create a workspace domain.
* [get](docs/sdks/domains/README.md#get) - Get domain by ID.
* [delete](docs/sdks/domains/README.md#delete) - Delete a workspace domain.

### [Events](docs/sdks/events/README.md)

* [list](docs/sdks/events/README.md#list) - Retrieve all events.
* [get](docs/sdks/events/README.md#get) - Retrieve an event by ID.

### [Managers](docs/sdks/managers/README.md)

* [list](docs/sdks/managers/README.md#list) - Retrieve all managers.
* [create](docs/sdks/managers/README.md#create) - Create a new manager.
* [get](docs/sdks/managers/README.md#get) - Retrieve a manager by ID.
* [delete](docs/sdks/managers/README.md#delete) - Delete a manager by ID.
* [getManagementConfig](docs/sdks/managers/README.md#getmanagementconfig) - Get the management configuration for a manager.
* [provision](docs/sdks/managers/README.md#provision) - Enqueue provisioning for a manager by ID.
* [update](docs/sdks/managers/README.md#update) - Update a manager to a specific release ID or active release.
* [listEvents](docs/sdks/managers/README.md#listevents) - Retrieve all events of a manager.
* [generateDeepstoreToken](docs/sdks/managers/README.md#generatedeepstoretoken) - Generate a JWT token and connection info for querying manager logs directly. Returns everything the browser needs to create a DeepstoreClient and query the data plane without routing log data through the platform API (end-to-end encryption).
* [reportHeartbeat](docs/sdks/managers/README.md#reportheartbeat) - Report Manager health status and metrics.
* [getDeployment](docs/sdks/managers/README.md#getdeployment) - Get deployment details for a private manager (internal deployment platform, status, resources).
* [googleCloudConnect](docs/sdks/managers/README.md#googlecloudconnect) - Connect a Google Cloud manager via OAuth.

### [Packages](docs/sdks/packages/README.md)

* [list](docs/sdks/packages/README.md#list) - List packages with optional filters. Returns packages ordered by creation date (newest first).
* [get](docs/sdks/packages/README.md#get) - Get details of a specific package.
* [rebuild](docs/sdks/packages/README.md#rebuild) - Rebuild packages for a project. This will cancel any pending packages and create new ones with auto-incremented versions.
* [cancel](docs/sdks/packages/README.md#cancel) - Cancel a pending or building package.

### [Projects](docs/sdks/projects/README.md)

* [list](docs/sdks/projects/README.md#list) - Retrieve all projects.
* [create](docs/sdks/projects/README.md#create) - Create a new project.
* [get](docs/sdks/projects/README.md#get) - Retrieve a project by ID or name.
* [delete](docs/sdks/projects/README.md#delete) - Delete a project. The project must have no agents.
* [update](docs/sdks/projects/README.md#update) - Update a project.
* [createFromTemplate](docs/sdks/projects/README.md#createfromtemplate) - Create a project by forking alienplatform/alien into your namespace, then configuring GitHub Actions.
* [getTemplateUrls](docs/sdks/projects/README.md#gettemplateurls) - Get template URLs for deploying agents in this project.
* [getActiveRelease](docs/sdks/projects/README.md#getactiverelease) - Get the active release for this project. Returns the latest release, or the pinned release if deploymentId is provided and that deployment has a pinned release.

### [Releases](docs/sdks/releases/README.md)

* [list](docs/sdks/releases/README.md#list) - Retrieve all releases.
* [create](docs/sdks/releases/README.md#create) - Create a new release.
* [listBranches](docs/sdks/releases/README.md#listbranches) - List distinct git branches across releases. Used for filter dropdowns.
* [listAuthors](docs/sdks/releases/README.md#listauthors) - List distinct commit authors across releases. Used for filter dropdowns.
* [get](docs/sdks/releases/README.md#get) - Retrieve a release by ID.

### [Resolve](docs/sdks/resolve/README.md)

* [resolve](docs/sdks/resolve/README.md#resolve) - Resolve manager for a project and platform

### [Sync](docs/sdks/sync/README.md)

* [acquire](docs/sdks/sync/README.md#acquire) - Acquire a batch of deployments for processing. Used by Manager to atomically lock deployments matching filters. Each deployment in the batch must be released after processing.
* [reconcile](docs/sdks/sync/README.md#reconcile) - Reconcile agent deployment state. Push model (with session) verifies lock ownership. Pull model (no session) verifies agent is unlocked. Accepts full DeploymentState after step() execution.
* [release](docs/sdks/sync/README.md#release) - Release an agent's deployment lock. Must be called after processing an acquired agent, even if processing failed. This is critical to avoid deadlocks.

### [User](docs/sdks/user/README.md)

* [updateProfile](docs/sdks/user/README.md#updateprofile) - Update the current user's profile (display name).
* [listMemberships](docs/sdks/user/README.md#listmemberships) - List all workspaces the current user has access to.
* [createWorkspace](docs/sdks/user/README.md#createworkspace) - Create a new workspace. The current user will be automatically added as an admin.
* [listGitNamespaces](docs/sdks/user/README.md#listgitnamespaces) - List all git namespaces (GitHub installations) the current user has access to.
* [syncGitNamespaces](docs/sdks/user/README.md#syncgitnamespaces) - Sync git namespaces from the provider. For GitHub, this fetches all app installations accessible to the user.
* [listGitNamespaceRepositories](docs/sdks/user/README.md#listgitnamespacerepositories) - List repositories accessible through a git namespace (GitHub installation).

### [Workspaces](docs/sdks/workspaces/README.md)

* [list](docs/sdks/workspaces/README.md#list) - Retrieve all workspaces.
* [get](docs/sdks/workspaces/README.md#get) - Retrieve a workspace by ID.
* [delete](docs/sdks/workspaces/README.md#delete) - Delete a workspace. The workspace must have no projects.
* [update](docs/sdks/workspaces/README.md#update) - Update a workspace.
* [listMembers](docs/sdks/workspaces/README.md#listmembers) - List all members of a workspace.
* [addMember](docs/sdks/workspaces/README.md#addmember) - Add a member to a workspace by email. The user must already have an account.
* [removeMember](docs/sdks/workspaces/README.md#removemember) - Remove a member from a workspace.
* [updateMember](docs/sdks/workspaces/README.md#updatemember) - Update a workspace member's role.

</details>
<!-- End Available Resources and Operations [operations] -->

<!-- Start Standalone functions [standalone-funcs] -->
## Standalone functions

All the methods listed above are available as standalone functions. These
functions are ideal for use in applications running in the browser, serverless
runtimes or other environments where application bundle size is a primary
concern. When using a bundler to build your application, all unused
functionality will be either excluded from the final bundle or tree-shaken away.

To read more about standalone functions, check [FUNCTIONS.md](./FUNCTIONS.md).

<details>

<summary>Available standalone functions</summary>

- [`apiKeysCreate`](docs/sdks/apikeys/README.md#create) - Create a new API key.
- [`apiKeysDeleteMultiple`](docs/sdks/apikeys/README.md#deletemultiple) - Permanently delete multiple API keys.
- [`apiKeysGet`](docs/sdks/apikeys/README.md#get) - Retrieve a specific API key.
- [`apiKeysList`](docs/sdks/apikeys/README.md#list) - Retrieve all API keys for the current workspace.
- [`apiKeysRevoke`](docs/sdks/apikeys/README.md#revoke) - Revoke (soft delete) an API key.
- [`apiKeysUpdate`](docs/sdks/apikeys/README.md#update) - Update an API key (enable/disable, change description).
- [`authWhoami`](docs/sdks/auth/README.md#whoami) - Get current authenticated principal information (user or service account). Works with both session cookies and API keys.
- [`commandsCreate`](docs/sdks/commands/README.md#create) - Create command metadata. Called by manager when processing commands. Returns project info for routing decisions.
- [`commandsGet`](docs/sdks/commands/README.md#get) - Retrieve a command by ID.
- [`commandsList`](docs/sdks/commands/README.md#list) - Retrieve commands. Use for dashboard analytics and command history.
- [`commandsListDeployments`](docs/sdks/commands/README.md#listdeployments) - List distinct deployments that have commands, including deployment group info. Use for filter dropdowns in the dashboard.
- [`commandsListNames`](docs/sdks/commands/README.md#listnames) - List distinct command names. Use for filter dropdowns in the dashboard.
- [`commandsUpdate`](docs/sdks/commands/README.md#update) - Update command state. Called by manager when command is dispatched or completes.
- [`containersGetAttention`](docs/sdks/containers/README.md#getattention) - Returns deployments that need attention: crash loops, scheduling failures, unhealthy machines.
- [`containersGetDeploymentCluster`](docs/sdks/containers/README.md#getdeploymentcluster) - Container cluster overview for a specific deployment: machine count, container count, and capacity.
- [`containersGetDeploymentContainer`](docs/sdks/containers/README.md#getdeploymentcontainer) - Get detailed status, configuration, and replica metrics for a specific container in a deployment.
- [`containersGetDeployments`](docs/sdks/containers/README.md#getdeployments) - Per-deployment breakdown for a container: status, replicas, metrics, and HTTP performance across all deployments running this container.
- [`containersGetMachines`](docs/sdks/containers/README.md#getmachines) - Cross-deployment machine health: per-deployment machine counts by status, capacity group utilization, and scaling recommendations.
- [`containersGetOverview`](docs/sdks/containers/README.md#getoverview) - Bird's-eye view of all container definitions across all deployments, with aggregate health stats, machine health breakdown, and HTTP performance metrics.
- [`containersListDeploymentContainerInstanceEvents`](docs/sdks/containers/README.md#listdeploymentcontainerinstanceevents) - List orchestration events for a specific container in a deployment.
- [`containersListDeploymentContainers`](docs/sdks/containers/README.md#listdeploymentcontainers) - List all containers running in a specific deployment.
- [`containersListDeploymentEvents`](docs/sdks/containers/README.md#listdeploymentevents) - List orchestration events for a deployment's container cluster.
- [`containersListDeploymentMachines`](docs/sdks/containers/README.md#listdeploymentmachines) - List all compute machines in a deployment's container cluster.
- [`deploymentGetInfo`](docs/sdks/deployment/README.md#getinfo) - Get deployment information for the deployment page. Accepts both agent-scoped and deployment-group-scoped API keys. Returns project information, package status/outputs, and either agent or deployment group details depending on the token type. Poll this endpoint to check if packages are ready.
- [`deploymentGroupsCreateDeploymentGroup`](docs/sdks/deploymentgroups/README.md#createdeploymentgroup) - Create a new deployment group
- [`deploymentGroupsCreateDeploymentGroupToken`](docs/sdks/deploymentgroups/README.md#createdeploymentgrouptoken) - Create deployment group token
- [`deploymentGroupsDeleteDeploymentGroup`](docs/sdks/deploymentgroups/README.md#deletedeploymentgroup) - Delete deployment group
- [`deploymentGroupsGetDeploymentGroup`](docs/sdks/deploymentgroups/README.md#getdeploymentgroup) - Get deployment group details
- [`deploymentGroupsListDeploymentGroups`](docs/sdks/deploymentgroups/README.md#listdeploymentgroups) - List deployment groups
- [`deploymentGroupsUpdateDeploymentGroup`](docs/sdks/deploymentgroups/README.md#updatedeploymentgroup) - Update deployment group
- [`deploymentsCreate`](docs/sdks/deployments/README.md#create) - Create a new agent. Deployment group tokens automatically use their group. Workspace/project tokens must provide deploymentGroupId.
- [`deploymentsCreateToken`](docs/sdks/deployments/README.md#createtoken) - Create an agent token (agent-scoped API key) for this agent. The agent must exist before creating a token.
- [`deploymentsDelete`](docs/sdks/deployments/README.md#delete) - Delete an agent by ID. This can be used to start deletion or retry failed deletions.
- [`deploymentsGet`](docs/sdks/deployments/README.md#get) - Retrieve an agent by ID.
- [`deploymentsGetInfo`](docs/sdks/deployments/README.md#getinfo) - Get deployment connection information including command endpoint and resource URLs.
- [`deploymentsGetStats`](docs/sdks/deployments/README.md#getstats) - Get aggregated deployment statistics. Returns total count and breakdown by status.
- [`deploymentsImport`](docs/sdks/deployments/README.md#import) - Import an agent from existing infrastructure (e.g., CloudFormation stack). The agent ID is automatically generated.
- [`deploymentsList`](docs/sdks/deployments/README.md#list) - Retrieve all deployments.
- [`deploymentsListFilterDeploymentGroups`](docs/sdks/deployments/README.md#listfilterdeploymentgroups) - List deployment groups with agent counts. Used for filter dropdowns.
- [`deploymentsListFilterPlatforms`](docs/sdks/deployments/README.md#listfilterplatforms) - List distinct platforms used by deployments. Used for filter dropdowns.
- [`deploymentsPinRelease`](docs/sdks/deployments/README.md#pinrelease) - Pin or unpin deployment to a specific release. Only works for running deployments. Controller will automatically trigger update to target release.
- [`deploymentsRedeploy`](docs/sdks/deployments/README.md#redeploy) - Redeploy a running agent with the same release and fresh environment variables. Sets status to update-pending.
- [`deploymentsRetry`](docs/sdks/deployments/README.md#retry) - Retry a failed agent operation. Uses alien-infra's retry mechanisms to resume from exact failure point.
- [`deploymentsUpdateEnvironmentVariables`](docs/sdks/deployments/README.md#updateenvironmentvariables) - Update an agent's environment variables. If the agent is running and not locked, the status will be changed to update-pending to trigger a deployment.
- [`domainsCreate`](docs/sdks/domains/README.md#create) - Create a workspace domain.
- [`domainsDelete`](docs/sdks/domains/README.md#delete) - Delete a workspace domain.
- [`domainsGet`](docs/sdks/domains/README.md#get) - Get domain by ID.
- [`domainsList`](docs/sdks/domains/README.md#list) - List system domains and workspace domains.
- [`eventsGet`](docs/sdks/events/README.md#get) - Retrieve an event by ID.
- [`eventsList`](docs/sdks/events/README.md#list) - Retrieve all events.
- [`managersCreate`](docs/sdks/managers/README.md#create) - Create a new manager.
- [`managersDelete`](docs/sdks/managers/README.md#delete) - Delete a manager by ID.
- [`managersGenerateDeepstoreToken`](docs/sdks/managers/README.md#generatedeepstoretoken) - Generate a JWT token and connection info for querying manager logs directly. Returns everything the browser needs to create a DeepstoreClient and query the data plane without routing log data through the platform API (end-to-end encryption).
- [`managersGet`](docs/sdks/managers/README.md#get) - Retrieve a manager by ID.
- [`managersGetDeployment`](docs/sdks/managers/README.md#getdeployment) - Get deployment details for a private manager (internal deployment platform, status, resources).
- [`managersGetManagementConfig`](docs/sdks/managers/README.md#getmanagementconfig) - Get the management configuration for a manager.
- [`managersGoogleCloudConnect`](docs/sdks/managers/README.md#googlecloudconnect) - Connect a Google Cloud manager via OAuth.
- [`managersList`](docs/sdks/managers/README.md#list) - Retrieve all managers.
- [`managersListEvents`](docs/sdks/managers/README.md#listevents) - Retrieve all events of a manager.
- [`managersProvision`](docs/sdks/managers/README.md#provision) - Enqueue provisioning for a manager by ID.
- [`managersReportHeartbeat`](docs/sdks/managers/README.md#reportheartbeat) - Report Manager health status and metrics.
- [`managersUpdate`](docs/sdks/managers/README.md#update) - Update a manager to a specific release ID or active release.
- [`packagesCancel`](docs/sdks/packages/README.md#cancel) - Cancel a pending or building package.
- [`packagesGet`](docs/sdks/packages/README.md#get) - Get details of a specific package.
- [`packagesList`](docs/sdks/packages/README.md#list) - List packages with optional filters. Returns packages ordered by creation date (newest first).
- [`packagesRebuild`](docs/sdks/packages/README.md#rebuild) - Rebuild packages for a project. This will cancel any pending packages and create new ones with auto-incremented versions.
- [`projectsCreate`](docs/sdks/projects/README.md#create) - Create a new project.
- [`projectsCreateFromTemplate`](docs/sdks/projects/README.md#createfromtemplate) - Create a project by forking alienplatform/alien into your namespace, then configuring GitHub Actions.
- [`projectsDelete`](docs/sdks/projects/README.md#delete) - Delete a project. The project must have no agents.
- [`projectsGet`](docs/sdks/projects/README.md#get) - Retrieve a project by ID or name.
- [`projectsGetActiveRelease`](docs/sdks/projects/README.md#getactiverelease) - Get the active release for this project. Returns the latest release, or the pinned release if deploymentId is provided and that deployment has a pinned release.
- [`projectsGetTemplateUrls`](docs/sdks/projects/README.md#gettemplateurls) - Get template URLs for deploying agents in this project.
- [`projectsList`](docs/sdks/projects/README.md#list) - Retrieve all projects.
- [`projectsUpdate`](docs/sdks/projects/README.md#update) - Update a project.
- [`releasesCreate`](docs/sdks/releases/README.md#create) - Create a new release.
- [`releasesGet`](docs/sdks/releases/README.md#get) - Retrieve a release by ID.
- [`releasesList`](docs/sdks/releases/README.md#list) - Retrieve all releases.
- [`releasesListAuthors`](docs/sdks/releases/README.md#listauthors) - List distinct commit authors across releases. Used for filter dropdowns.
- [`releasesListBranches`](docs/sdks/releases/README.md#listbranches) - List distinct git branches across releases. Used for filter dropdowns.
- [`resolveResolve`](docs/sdks/resolve/README.md#resolve) - Resolve manager for a project and platform
- [`syncAcquire`](docs/sdks/sync/README.md#acquire) - Acquire a batch of deployments for processing. Used by Manager to atomically lock deployments matching filters. Each deployment in the batch must be released after processing.
- [`syncReconcile`](docs/sdks/sync/README.md#reconcile) - Reconcile agent deployment state. Push model (with session) verifies lock ownership. Pull model (no session) verifies agent is unlocked. Accepts full DeploymentState after step() execution.
- [`syncRelease`](docs/sdks/sync/README.md#release) - Release an agent's deployment lock. Must be called after processing an acquired agent, even if processing failed. This is critical to avoid deadlocks.
- [`userCreateWorkspace`](docs/sdks/user/README.md#createworkspace) - Create a new workspace. The current user will be automatically added as an admin.
- [`userListGitNamespaceRepositories`](docs/sdks/user/README.md#listgitnamespacerepositories) - List repositories accessible through a git namespace (GitHub installation).
- [`userListGitNamespaces`](docs/sdks/user/README.md#listgitnamespaces) - List all git namespaces (GitHub installations) the current user has access to.
- [`userListMemberships`](docs/sdks/user/README.md#listmemberships) - List all workspaces the current user has access to.
- [`userSyncGitNamespaces`](docs/sdks/user/README.md#syncgitnamespaces) - Sync git namespaces from the provider. For GitHub, this fetches all app installations accessible to the user.
- [`userUpdateProfile`](docs/sdks/user/README.md#updateprofile) - Update the current user's profile (display name).
- [`workspacesAddMember`](docs/sdks/workspaces/README.md#addmember) - Add a member to a workspace by email. The user must already have an account.
- [`workspacesDelete`](docs/sdks/workspaces/README.md#delete) - Delete a workspace. The workspace must have no projects.
- [`workspacesGet`](docs/sdks/workspaces/README.md#get) - Retrieve a workspace by ID.
- [`workspacesList`](docs/sdks/workspaces/README.md#list) - Retrieve all workspaces.
- [`workspacesListMembers`](docs/sdks/workspaces/README.md#listmembers) - List all members of a workspace.
- [`workspacesRemoveMember`](docs/sdks/workspaces/README.md#removemember) - Remove a member from a workspace.
- [`workspacesUpdate`](docs/sdks/workspaces/README.md#update) - Update a workspace.
- [`workspacesUpdateMember`](docs/sdks/workspaces/README.md#updatemember) - Update a workspace member's role.

</details>
<!-- End Standalone functions [standalone-funcs] -->

<!-- Start Retries [retries] -->
## Retries

Some of the endpoints in this SDK support retries.  If you use the SDK without any configuration, it will fall back to the default retry strategy provided by the API.  However, the default retry strategy can be overridden on a per-operation basis, or across the entire SDK.

To change the default retry strategy for a single API call, simply provide a retryConfig object to the call:
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.user.updateProfile({
    retries: {
      strategy: "backoff",
      backoff: {
        initialInterval: 1,
        maxInterval: 50,
        exponent: 1.1,
        maxElapsedTime: 100,
      },
      retryConnectionErrors: false,
    },
  });

  console.log(result);
}

run();

```

If you'd like to override the default retry strategy for all operations that support retries, you can provide a retryConfig at SDK initialization:
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  retryConfig: {
    strategy: "backoff",
    backoff: {
      initialInterval: 1,
      maxInterval: 50,
      exponent: 1.1,
      maxElapsedTime: 100,
    },
    retryConnectionErrors: false,
  },
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.user.updateProfile();

  console.log(result);
}

run();

```
<!-- End Retries [retries] -->

<!-- Start Error Handling [errors] -->
## Error Handling

[`AlienError`](./src/models/errors/alienerror.ts) is the base class for all HTTP error responses. It has the following properties:

| Property            | Type       | Description                                                                             |
| ------------------- | ---------- | --------------------------------------------------------------------------------------- |
| `error.message`     | `string`   | Error message                                                                           |
| `error.statusCode`  | `number`   | HTTP response status code eg `404`                                                      |
| `error.headers`     | `Headers`  | HTTP response headers                                                                   |
| `error.body`        | `string`   | HTTP body. Can be empty string if no body is returned.                                  |
| `error.rawResponse` | `Response` | Raw HTTP response                                                                       |
| `error.data$`       |            | Optional. Some errors may contain structured data. [See Error Classes](#error-classes). |

### Example
```typescript
import { Alien } from "@alienplatform/platform-api";
import * as errors from "@alienplatform/platform-api/models/errors";

const alien = new Alien({
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  try {
    const result = await alien.user.updateProfile();

    console.log(result);
  } catch (error) {
    // The base class for HTTP error responses
    if (error instanceof errors.AlienError) {
      console.log(error.message);
      console.log(error.statusCode);
      console.log(error.body);
      console.log(error.headers);

      // Depending on the method different errors may be thrown
      if (error instanceof errors.APIError) {
        console.log(error.data$.code); // string
        console.log(error.data$.message); // string
        console.log(error.data$.source); // any
        console.log(error.data$.retryable); // boolean
        console.log(error.data$.context); // any
      }
    }
  }
}

run();

```

### Error Classes
**Primary errors:**
* [`AlienError`](./src/models/errors/alienerror.ts): The base class for HTTP error responses.
  * [`APIError`](./src/models/errors/apierror.ts): Generic error.

<details><summary>Less common errors (6)</summary>

<br />

**Network errors:**
* [`ConnectionError`](./src/models/errors/httpclienterrors.ts): HTTP client was unable to make a request to a server.
* [`RequestTimeoutError`](./src/models/errors/httpclienterrors.ts): HTTP request timed out due to an AbortSignal signal.
* [`RequestAbortedError`](./src/models/errors/httpclienterrors.ts): HTTP request was aborted by the client.
* [`InvalidRequestError`](./src/models/errors/httpclienterrors.ts): Any input used to create a request is invalid.
* [`UnexpectedClientError`](./src/models/errors/httpclienterrors.ts): Unrecognised or unexpected error.


**Inherit from [`AlienError`](./src/models/errors/alienerror.ts)**:
* [`ResponseValidationError`](./src/models/errors/responsevalidationerror.ts): Type mismatch between the data returned from the server and the structure expected by the SDK. See `error.rawValue` for the raw value and `error.pretty()` for a nicely formatted multi-line string.

</details>
<!-- End Error Handling [errors] -->

<!-- Start Server Selection [server] -->
## Server Selection

### Override Server URL Per-Client

The default server can be overridden globally by passing a URL to the `serverURL: string` optional parameter when initializing the SDK client instance. For example:
```typescript
import { Alien } from "@alienplatform/platform-api";

const alien = new Alien({
  serverURL: "https://api.alien.dev",
  apiKey: process.env["ALIEN_API_KEY"] ?? "",
});

async function run() {
  const result = await alien.user.updateProfile();

  console.log(result);
}

run();

```
<!-- End Server Selection [server] -->

<!-- Start Custom HTTP Client [http-client] -->
## Custom HTTP Client

The TypeScript SDK makes API calls using an `HTTPClient` that wraps the native
[Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API). This
client is a thin wrapper around `fetch` and provides the ability to attach hooks
around the request lifecycle that can be used to modify the request or handle
errors and response.

The `HTTPClient` constructor takes an optional `fetcher` argument that can be
used to integrate a third-party HTTP client or when writing tests to mock out
the HTTP client and feed in fixtures.

The following example shows how to use the `"beforeRequest"` hook to to add a
custom header and a timeout to requests and how to use the `"requestError"` hook
to log errors:

```typescript
import { Alien } from "@alienplatform/platform-api";
import { HTTPClient } from "@alienplatform/platform-api/lib/http";

const httpClient = new HTTPClient({
  // fetcher takes a function that has the same signature as native `fetch`.
  fetcher: (request) => {
    return fetch(request);
  }
});

httpClient.addHook("beforeRequest", (request) => {
  const nextRequest = new Request(request, {
    signal: request.signal || AbortSignal.timeout(5000)
  });

  nextRequest.headers.set("x-custom-header", "custom value");

  return nextRequest;
});

httpClient.addHook("requestError", (error, request) => {
  console.group("Request Error");
  console.log("Reason:", `${error}`);
  console.log("Endpoint:", `${request.method} ${request.url}`);
  console.groupEnd();
});

const sdk = new Alien({ httpClient: httpClient });
```
<!-- End Custom HTTP Client [http-client] -->

<!-- Start Debugging [debug] -->
## Debugging

You can setup your SDK to emit debug logs for SDK requests and responses.

You can pass a logger that matches `console`'s interface as an SDK option.

> [!WARNING]
> Beware that debug logging will reveal secrets, like API tokens in headers, in log messages printed to a console or files. It's recommended to use this feature only during local development and not in production.

```typescript
import { Alien } from "@alienplatform/platform-api";

const sdk = new Alien({ debugLogger: console });
```

You can also enable a default debug logger by setting an environment variable `ALIEN_DEBUG` to true.
<!-- End Debugging [debug] -->

<!-- Placeholder for Future Speakeasy SDK Sections -->

# Development

## Maturity

This SDK is in beta, and there may be breaking changes between versions without a major version update. Therefore, we recommend pinning usage
to a specific package version. This way, you can install the same version each time without breaking changes unless you are intentionally
looking for the latest version.

## Contributions

While we value open-source contributions to this SDK, this library is generated programmatically. Any manual changes added to internal files will be overwritten on the next generation. 
We look forward to hearing your feedback. Feel free to open a PR or an issue with a proof of concept and we'll do our best to include it in a future release. 

### SDK Created by [Speakeasy](https://www.speakeasy.com/?utm_source=openapi&utm_campaign=typescript)
