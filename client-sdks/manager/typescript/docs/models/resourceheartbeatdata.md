# ResourceHeartbeatData


## Supported Types

### `models.ResourceHeartbeatDataStorage`

```typescript
const value: models.ResourceHeartbeatDataStorage = {
  data: {
    path: "/Library",
    pathExists: true,
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "stopping",
      partial: true,
      stale: true,
    },
    backend: "local",
  },
  resourceType: "storage",
};
```

### `models.ResourceHeartbeatDataWorker`

```typescript
const value: models.ResourceHeartbeatDataWorker = {
  data: {
    appName: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
    backend: "azureContainerApps",
  },
  resourceType: "worker",
};
```

### `models.ResourceHeartbeatDataContainer`

```typescript
const value: models.ResourceHeartbeatDataContainer = {
  data: {
    events: [
      {
        message: "<value>",
        reason: "<value>",
      },
    ],
    name: "<value>",
    namespace: "<value>",
    pods: [],
    replicas: {},
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
    workloadKind: "daemonSet",
    backend: "kubernetes",
  },
  resourceType: "container",
};
```

### `models.ResourceHeartbeatDataDaemon`

```typescript
const value: models.ResourceHeartbeatDataDaemon = {
  data: {
    assignedMachines: 351239,
    capacityGroup: "<value>",
    commandSupported: true,
    daemonInstances: [
      {
        name: "<value>",
        ready: false,
        replicaId: "<id>",
      },
    ],
    desiredMachines: 920664,
    events: [],
    healthyInstances: 222122,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    latestUpdateTimestamp: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
    unavailableInstances: 631428,
    backend: "azure",
  },
  resourceType: "daemon",
};
```

### `models.ResourceHeartbeatDataComputeCluster`

```typescript
const value: models.ResourceHeartbeatDataComputeCluster = {
  data: {
    dockerAvailable: true,
    name: "<value>",
    networkAvailable: false,
    nodes: {},
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: false,
      stale: true,
    },
    backend: "local",
  },
  resourceType: "compute-cluster",
};
```

### `models.ResourceHeartbeatDataKubernetesCluster`

```typescript
const value: models.ResourceHeartbeatDataKubernetesCluster = {
  data: {
    events: [],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

### `models.ResourceHeartbeatDataQueue`

```typescript
const value: models.ResourceHeartbeatDataQueue = {
  data: {
    messageStorageAllowedPersistenceRegions: [
      "<value 1>",
    ],
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "deleted",
      partial: true,
      stale: false,
    },
    subscriptionLabels: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    subscriptionPushAttributes: {},
    topicLabels: {
      "key": "<value>",
      "key1": "<value>",
    },
    topicName: "<value>",
    backend: "gcpPubSub",
  },
  resourceType: "queue",
};
```

### `models.ResourceHeartbeatDataKv`

```typescript
const value: models.ResourceHeartbeatDataKv = {
  data: {
    keySchema: [],
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "creating",
      partial: false,
      stale: false,
    },
    backend: "awsDynamoDb",
  },
  resourceType: "kv",
};
```

### `models.ResourceHeartbeatDataVault`

```typescript
const value: models.ResourceHeartbeatDataVault = {
  data: {
    namespace: "<value>",
    prefix: "<value>",
    secretMetadataListed: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "scaling",
      partial: true,
      stale: true,
    },
    backend: "kubernetesSecret",
  },
  resourceType: "vault",
};
```

### `models.ResourceHeartbeatDataServiceAccount`

```typescript
const value: models.ResourceHeartbeatDataServiceAccount = {
  data: {
    customRoleDefinitionCount: 783312,
    customRoleDefinitionIds: [
      "<value 1>",
    ],
    location: "<value>",
    managedTagCount: 22826,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 127599,
    roleAssignmentIds: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    stackPermissionsApplied: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: false,
      stale: true,
    },
    backend: "azureManagedIdentity",
  },
  resourceType: "service-account",
};
```

### `models.ResourceHeartbeatDataNetwork`

```typescript
const value: models.ResourceHeartbeatDataNetwork = {
  data: {
    isByoVpc: true,
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "stopped",
      partial: true,
      stale: true,
    },
    backend: "gcpVpc",
  },
  resourceType: "network",
};
```

### `models.ResourceHeartbeatDataRemoteStackManagement`

```typescript
const value: models.ResourceHeartbeatDataRemoteStackManagement = {
  data: {
    managementPermissionsApplied: true,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "running",
      partial: true,
      stale: true,
    },
    backend: "awsIamRole",
  },
  resourceType: "remote-stack-management",
};
```

### `models.ResourceHeartbeatDataArtifactRegistry`

```typescript
const value: models.ResourceHeartbeatDataArtifactRegistry = {
  data: {
    region: "<value>",
    registryId: "<id>",
    registryUri: "https://dead-minor.info/",
    repositories: [],
    repositoriesTruncated: false,
    repositoryCount: 294854,
    repositoryPrefix: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "scaling",
      partial: true,
      stale: false,
    },
    backend: "awsEcr",
  },
  resourceType: "artifact-registry",
};
```

### `models.ResourceHeartbeatDataBuild`

```typescript
const value: models.ResourceHeartbeatDataBuild = {
  data: {
    environmentVariableCount: 816046,
    managedEnvironmentId: "<id>",
    resourceGroupName: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "updating",
      partial: false,
      stale: false,
    },
    backend: "azureContainerApps",
  },
  resourceType: "build",
};
```

### `models.ResourceHeartbeatDataServiceActivation`

```typescript
const value: models.ResourceHeartbeatDataServiceActivation = {
  data: {
    namespace: "<value>",
    registered: true,
    resourceTypeCount: 100188,
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "unknown",
      partial: false,
      stale: true,
    },
    backend: "azureResourceProvider",
  },
  resourceType: "service_activation",
};
```

### `models.ResourceHeartbeatDataAzureResourceGroup`

```typescript
const value: models.ResourceHeartbeatDataAzureResourceGroup = {
  data: {
    managedTags: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "updating",
      partial: false,
      stale: true,
    },
  },
  resourceType: "azure_resource_group",
};
```

### `models.ResourceHeartbeatDataAzureStorageAccount`

```typescript
const value: models.ResourceHeartbeatDataAzureStorageAccount = {
  data: {
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "stopping",
      partial: true,
      stale: true,
    },
  },
  resourceType: "azure_storage_account",
};
```

### `models.ResourceHeartbeatDataAzureContainerAppsEnvironment`

```typescript
const value: models.ResourceHeartbeatDataAzureContainerAppsEnvironment = {
  data: {
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "scaling",
      partial: true,
      stale: false,
    },
    workloadProfileCount: 324464,
    workloadProfiles: [
      {
        name: "<value>",
        workloadProfileType: "<value>",
      },
    ],
  },
  resourceType: "azure_container_apps_environment",
};
```

### `models.ResourceHeartbeatDataAzureServiceBusNamespace`

```typescript
const value: models.ResourceHeartbeatDataAzureServiceBusNamespace = {
  data: {
    name: "<value>",
    privateEndpointConnectionCount: 24724,
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "deleted",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```

