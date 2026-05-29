# DataUnion15


## Supported Types

### `operations.DataStorage`

```typescript
const value: operations.DataStorage = {
  data: {
    events: [],
    path: "/var/mail",
    pathExists: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "deleting",
      partial: true,
      stale: true,
    },
    backend: "local",
  },
  resourceType: "storage",
};
```

### `operations.DataWorker`

```typescript
const value: operations.DataWorker = {
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
          reason: "forbidden",
          severity: "warning",
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

### `operations.DataContainer`

```typescript
const value: operations.DataContainer = {
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
          severity: "warning",
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

### `operations.DataDaemon`

```typescript
const value: operations.DataDaemon = {
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
          reason: "collection-failed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
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

### `operations.DataComputeCluster`

```typescript
const value: operations.DataComputeCluster = {
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

### `operations.DataKubernetesCluster`

```typescript
const value: operations.DataKubernetesCluster = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-08-02T12:07:39.617Z"),
        severity: "error",
      },
    ],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopping",
      partial: false,
      stale: false,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

### `operations.DataQueue`

```typescript
const value: operations.DataQueue = {
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

### `operations.DataKv`

```typescript
const value: operations.DataKv = {
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
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "failed",
      partial: true,
      stale: true,
    },
    storageAccountName: "<value>",
    tableExists: true,
    tableName: "<value>",
    backend: "azureTable",
  },
  resourceType: "kv",
};
```

### `operations.DataVault`

```typescript
const value: operations.DataVault = {
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

### `operations.DataServiceAccount`

```typescript
const value: operations.DataServiceAccount = {
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

### `operations.DataNetwork`

```typescript
const value: operations.DataNetwork = {
  data: {
    events: [],
    isByoVnet: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "scaling",
      partial: true,
      stale: false,
    },
    backend: "azureVnet",
  },
  resourceType: "network",
};
```

### `operations.DataRemoteStackManagement`

```typescript
const value: operations.DataRemoteStackManagement = {
  data: {
    events: [],
    managementPermissionsApplied: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
    backend: "awsIamRole",
  },
  resourceType: "remote-stack-management",
};
```

### `operations.DataArtifactRegistry`

```typescript
const value: operations.DataArtifactRegistry = {
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

### `operations.DataBuild`

```typescript
const value: operations.DataBuild = {
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

### `operations.DataServiceActivation`

```typescript
const value: operations.DataServiceActivation = {
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
          severity: "info",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "stopping",
      partial: false,
      stale: true,
    },
    backend: "gcpServiceUsage",
  },
  resourceType: "service_activation",
};
```

### `operations.DataAzureResourceGroup`

```typescript
const value: operations.DataAzureResourceGroup = {
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
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "creating",
      partial: true,
      stale: true,
    },
  },
  resourceType: "azure_resource_group",
};
```

### `operations.DataAzureStorageAccount`

```typescript
const value: operations.DataAzureStorageAccount = {
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
          reason: "timed-out",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "deleted",
      partial: false,
      stale: true,
    },
  },
  resourceType: "azure_storage_account",
};
```

### `operations.DataAzureContainerAppsEnvironment`

```typescript
const value: operations.DataAzureContainerAppsEnvironment = {
  data: {
    events: [],
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "updating",
      partial: true,
      stale: true,
    },
    workloadProfileCount: 388415,
    workloadProfiles: [],
  },
  resourceType: "azure_container_apps_environment",
};
```

### `operations.DataAzureServiceBusNamespace`

```typescript
const value: operations.DataAzureServiceBusNamespace = {
  data: {
    events: [],
    name: "<value>",
    privateEndpointConnectionCount: 700227,
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```

