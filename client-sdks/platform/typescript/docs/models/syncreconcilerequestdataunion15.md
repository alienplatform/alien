# SyncReconcileRequestDataUnion15


## Supported Types

### `models.DataStorage`

```typescript
const value: models.DataStorage = {
  data: {
    events: [],
    path: "/var/mail",
    pathExists: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "stopping",
      partial: false,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "storage",
};
```

### `models.DataWorker`

```typescript
const value: models.DataWorker = {
  data: {
    commandSupported: false,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-05-06T03:27:45.769Z"),
        severity: "info",
      },
    ],
    imagePathPresent: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "updating",
      partial: true,
      stale: false,
    },
    triggerCount: 71218,
    backend: "local",
  },
  resourceType: "worker",
};
```

### `models.DataContainer`

```typescript
const value: models.DataContainer = {
  data: {
    attentionCount: 486054,
    containerId: "<id>",
    events: [],
    replicas: {},
    schedulingMode: "replicated",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "running",
      partial: true,
      stale: false,
    },
    backend: "horizonPlatform",
  },
  resourceType: "container",
};
```

### `models.DataDaemon`

```typescript
const value: models.DataDaemon = {
  data: {
    assignedMachines: 489905,
    capacityGroup: "<value>",
    commandSupported: true,
    daemonName: "<value>",
    desiredMachines: 300440,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2025-12-12T10:53:03.618Z"),
        severity: "error",
      },
    ],
    healthyInstances: 431179,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    instances: [
      {
        name: "<value>",
        ready: true,
        replicaId: "<id>",
      },
    ],
    latestUpdateTimestamp: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    unavailableInstances: 833736,
    backend: "gcp",
  },
  resourceType: "daemon",
};
```

### `models.DataComputeCluster`

```typescript
const value: models.DataComputeCluster = {
  data: {
    dockerAvailable: true,
    events: [],
    name: "<value>",
    networkAvailable: true,
    nodes: {},
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "compute-cluster",
};
```

### `models.DataKubernetesCluster`

```typescript
const value: models.DataKubernetesCluster = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2026-06-21T07:51:22.353Z"),
        severity: "info",
      },
    ],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

### `models.DataQueue`

```typescript
const value: models.DataQueue = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-08-12T10:51:43.799Z"),
        severity: "warning",
      },
    ],
    messageStorageAllowedPersistenceRegions: [
      "<value 1>",
    ],
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "updating",
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
    },
    topicName: "<value>",
    backend: "gcpPubSub",
  },
  resourceType: "queue",
};
```

### `models.DataKv`

```typescript
const value: models.DataKv = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2025-06-08T10:49:40.534Z"),
        severity: "warning",
      },
    ],
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "updating",
      partial: false,
      stale: false,
    },
    storageAccountName: "<value>",
    tableExists: true,
    tableName: "<value>",
    backend: "azureTable",
  },
  resourceType: "kv",
};
```

### `models.DataVault`

```typescript
const value: models.DataVault = {
  data: {
    accountId: "<id>",
    events: [],
    parameterMetadataSampled: true,
    prefix: "<value>",
    region: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "creating",
      partial: false,
      stale: false,
    },
    backend: "awsParameterStore",
  },
  resourceType: "vault",
};
```

### `models.DataServiceAccount`

```typescript
const value: models.DataServiceAccount = {
  data: {
    customRoleDefinitionCount: 991371,
    customRoleDefinitionIds: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    events: [],
    location: "<value>",
    managedTagCount: 703891,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 447985,
    roleAssignmentIds: [],
    stackPermissionsApplied: true,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    backend: "azureManagedIdentity",
  },
  resourceType: "service-account",
};
```

### `models.DataNetwork`

```typescript
const value: models.DataNetwork = {
  data: {
    events: [],
    isByoVnet: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "deleting",
      partial: false,
      stale: false,
    },
    backend: "azureVnet",
  },
  resourceType: "network",
};
```

### `models.DataRemoteStackManagement`

```typescript
const value: models.DataRemoteStackManagement = {
  data: {
    events: [],
    managementPermissionsApplied: true,
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
      lifecycle: "unknown",
      partial: false,
      stale: true,
    },
    backend: "awsIamRole",
  },
  resourceType: "remote-stack-management",
};
```

### `models.DataArtifactRegistry`

```typescript
const value: models.DataArtifactRegistry = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2026-05-25T20:34:34.708Z"),
        severity: "error",
      },
    ],
    reachable: false,
    registryUrl: "https://international-consistency.net/",
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "artifact-registry",
};
```

### `models.DataBuild`

```typescript
const value: models.DataBuild = {
  data: {
    encryptionKeyPresent: false,
    environmentVariableCount: 19119,
    events: [],
    projectName: "<value>",
    serviceRolePresent: false,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
    backend: "awsCodeBuild",
  },
  resourceType: "build",
};
```

### `models.DataServiceActivation`

```typescript
const value: models.DataServiceActivation = {
  data: {
    enabled: true,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2025-06-11T19:58:11.111Z"),
        severity: "error",
      },
    ],
    projectId: "<id>",
    serviceName: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "collection-failed",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "creating",
      partial: true,
      stale: false,
    },
    backend: "gcpServiceUsage",
  },
  resourceType: "service_activation",
};
```

### `models.DataAzureResourceGroup`

```typescript
const value: models.DataAzureResourceGroup = {
  data: {
    events: [],
    managedTags: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "updating",
      partial: true,
      stale: true,
    },
  },
  resourceType: "azure_resource_group",
};
```

### `models.DataAzureStorageAccount`

```typescript
const value: models.DataAzureStorageAccount = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-05-16T02:06:58.117Z"),
        severity: "warning",
      },
    ],
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "creating",
      partial: false,
      stale: true,
    },
  },
  resourceType: "azure_storage_account",
};
```

### `models.DataAzureContainerAppsEnvironment`

```typescript
const value: models.DataAzureContainerAppsEnvironment = {
  data: {
    events: [],
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "updating",
      partial: true,
      stale: false,
    },
    workloadProfileCount: 388415,
    workloadProfiles: [],
  },
  resourceType: "azure_container_apps_environment",
};
```

### `models.DataAzureServiceBusNamespace`

```typescript
const value: models.DataAzureServiceBusNamespace = {
  data: {
    events: [],
    name: "<value>",
    privateEndpointConnectionCount: 700227,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```

