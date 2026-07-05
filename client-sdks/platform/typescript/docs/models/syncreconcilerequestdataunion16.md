# SyncReconcileRequestDataUnion16


## Supported Types

### `models.DataStorage`

```typescript
const value: models.DataStorage = {
  data: {
    path: "/home",
    pathExists: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "collection-failed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopping",
      partial: false,
      stale: true,
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
        severity: "info",
        timestamp: new Date("2024-07-21T11:12:11.792Z"),
      },
    ],
    imagePathPresent: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
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

### `models.DataContainer`

```typescript
const value: models.DataContainer = {
  data: {
    attentionCount: 486054,
    containerId: "<id>",
    events: [],
    replicaUnits: [],
    replicas: {},
    schedulingMode: "stateful",
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
      lifecycle: "failed",
      partial: false,
      stale: true,
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
    daemonInstances: [],
    desiredMachines: 665477,
    events: [
      {
        message: "<value>",
        reason: "<value>",
      },
    ],
    healthyInstances: 921353,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    latestUpdateTimestamp: "<value>",
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
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    unavailableInstances: 431179,
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
    name: "<value>",
    networkAvailable: true,
    nodes: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
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
        message: "<value>",
        reason: "<value>",
      },
    ],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopping",
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
    messageStorageAllowedPersistenceRegions: [
      "<value 1>",
      "<value 2>",
    ],
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "failed",
      partial: true,
      stale: true,
    },
    subscriptionLabels: {
      "key": "<value>",
    },
    subscriptionPushAttributes: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    topicLabels: {},
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
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "scaling",
      partial: false,
      stale: true,
    },
    storageAccountName: "<value>",
    tableExists: false,
    tableName: "<value>",
    backend: "azureTable",
  },
  resourceType: "kv",
};
```

### `models.DataPostgres`

```typescript
const value: models.DataPostgres = {
  data: {
    serverName: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
    backend: "flexibleServer",
  },
  resourceType: "postgres",
};
```

### `models.DataVault`

```typescript
const value: models.DataVault = {
  data: {
    accountId: "<id>",
    parameterMetadataSampled: true,
    prefix: "<value>",
    region: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "unknown",
      partial: true,
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
    location: "<value>",
    managedTagCount: 438151,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 703891,
    roleAssignmentIds: [
      "<value 1>",
    ],
    stackPermissionsApplied: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "stopped",
      partial: true,
      stale: false,
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
    isByoVnet: true,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "unknown",
      partial: true,
      stale: true,
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
    managementPermissionsApplied: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopping",
      partial: true,
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
    reachable: false,
    registryUrl: "https://tedious-reach.com",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "creating",
      partial: true,
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
    projectName: "<value>",
    serviceRolePresent: true,
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
      lifecycle: "failed",
      partial: false,
      stale: true,
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
    projectId: "<id>",
    serviceName: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: true,
      stale: true,
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
    managedTags: {},
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "deleting",
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
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_storage_account",
};
```

### `models.DataAzureContainerAppsEnvironment`

```typescript
const value: models.DataAzureContainerAppsEnvironment = {
  data: {
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "scaling",
      partial: true,
      stale: true,
    },
    workloadProfileCount: 415280,
    workloadProfiles: [],
  },
  resourceType: "azure_container_apps_environment",
};
```

### `models.DataAzureServiceBusNamespace`

```typescript
const value: models.DataAzureServiceBusNamespace = {
  data: {
    name: "<value>",
    privateEndpointConnectionCount: 152029,
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "creating",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```
