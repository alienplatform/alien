# DataUnion16


## Supported Types

### `operations.DataStorage`

```typescript
const value: operations.DataStorage = {
  data: {
    path: "/home",
    pathExists: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "scaling",
      partial: false,
      stale: false,
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
        severity: "info",
        timestamp: new Date("2024-07-21T11:12:11.792Z"),
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
    replicaUnits: [],
    replicas: {},
    schedulingMode: "stateful",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "creating",
      partial: false,
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
          reason: "not-installed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: false,
    },
    unavailableInstances: 431179,
    backend: "azure",
  },
  resourceType: "daemon",
};
```

### `operations.DataComputeCluster`

```typescript
const value: operations.DataComputeCluster = {
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

### `operations.DataKubernetesCluster`

```typescript
const value: operations.DataKubernetesCluster = {
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
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "failed",
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

### `operations.DataKv`

```typescript
const value: operations.DataKv = {
  data: {
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
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

### `operations.DataPostgres`

```typescript
const value: operations.DataPostgres = {
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

### `operations.DataVault`

```typescript
const value: operations.DataVault = {
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
          reason: "not-installed",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "deleting",
      partial: true,
      stale: false,
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

### `operations.DataRemoteStackManagement`

```typescript
const value: operations.DataRemoteStackManagement = {
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
      health: "degraded",
      lifecycle: "stopping",
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

### `operations.DataBuild`

```typescript
const value: operations.DataBuild = {
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
          severity: "info",
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

### `operations.DataServiceActivation`

```typescript
const value: operations.DataServiceActivation = {
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

### `operations.DataAzureResourceGroup`

```typescript
const value: operations.DataAzureResourceGroup = {
  data: {
    managedTags: {},
    name: "<value>",
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
      lifecycle: "running",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_resource_group",
};
```

### `operations.DataAzureStorageAccount`

```typescript
const value: operations.DataAzureStorageAccount = {
  data: {
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: false,
      stale: false,
    },
  },
  resourceType: "azure_storage_account",
};
```

### `operations.DataAzureContainerAppsEnvironment`

```typescript
const value: operations.DataAzureContainerAppsEnvironment = {
  data: {
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
      stale: true,
    },
    workloadProfileCount: 415280,
    workloadProfiles: [],
  },
  resourceType: "azure_container_apps_environment",
};
```

### `operations.DataAzureServiceBusNamespace`

```typescript
const value: operations.DataAzureServiceBusNamespace = {
  data: {
    name: "<value>",
    privateEndpointConnectionCount: 152029,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
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

