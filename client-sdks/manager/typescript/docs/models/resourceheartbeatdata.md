# ResourceHeartbeatData


## Supported Types

### `models.ResourceHeartbeatDataStorage`

```typescript
const value: models.ResourceHeartbeatDataStorage = {
  data: {
    events: [],
    path: "/dev",
    pathExists: true,
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
      lifecycle: "updating",
      partial: true,
      stale: false,
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
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
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
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    instances: [],
    name: "<value>",
    namespace: "<value>",
    replicas: {},
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
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
    daemonName: "<value>",
    desiredMachines: 723101,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    healthyInstances: 920664,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    instances: [],
    latestUpdateTimestamp: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    unavailableInstances: 222122,
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
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    name: "<value>",
    networkAvailable: true,
    nodes: {},
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
      lifecycle: "deleted",
      partial: true,
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
      lifecycle: "running",
      partial: false,
      stale: true,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

### `models.ResourceHeartbeatDataQueue`

```typescript
const value: models.ResourceHeartbeatDataQueue = {
  data: {
    events: [],
    messageStorageAllowedPersistenceRegions: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "updating",
      partial: false,
      stale: false,
    },
    subscriptionLabels: {},
    subscriptionPushAttributes: {
      "key": "<value>",
      "key1": "<value>",
    },
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
    events: [],
    keySchema: [
      {
        attributeName: "<value>",
        keyType: "<value>",
      },
    ],
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopped",
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
    events: [],
    namespace: "<value>",
    prefix: "<value>",
    secretMetadataListed: false,
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
      lifecycle: "updating",
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
    events: [],
    location: "<value>",
    managedTagCount: 127599,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 916453,
    roleAssignmentIds: [],
    stackPermissionsApplied: false,
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
      lifecycle: "deleting",
      partial: true,
      stale: false,
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
    events: [],
    isByoVpc: true,
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "scaling",
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
    events: [],
    managementPermissionsApplied: true,
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
      lifecycle: "creating",
      partial: true,
      stale: false,
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
    events: [],
    region: "<value>",
    registryId: "<id>",
    registryUri: "https://orange-halt.biz/",
    repositories: [
      {
        createdAt: 2948.54,
        kmsKeyPresent: true,
        registryId: "<id>",
        repositoryArn: "<value>",
        repositoryName: "<value>",
        repositoryUri: "https://impolite-bran.name/",
      },
    ],
    repositoriesTruncated: false,
    repositoryCount: 16580,
    repositoryPrefix: "<value>",
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
      lifecycle: "stopped",
      partial: false,
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
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    managedEnvironmentId: "<id>",
    resourceGroupName: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
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
    events: [],
    namespace: "<value>",
    registered: true,
    resourceTypeCount: 440272,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopping",
      partial: true,
      stale: false,
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
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    managedTags: {},
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "deleted",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_resource_group",
};
```

### `models.ResourceHeartbeatDataAzureStorageAccount`

```typescript
const value: models.ResourceHeartbeatDataAzureStorageAccount = {
  data: {
    events: [],
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
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
      lifecycle: "updating",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_storage_account",
};
```

### `models.ResourceHeartbeatDataAzureContainerAppsEnvironment`

```typescript
const value: models.ResourceHeartbeatDataAzureContainerAppsEnvironment = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopping",
      partial: false,
      stale: false,
    },
    workloadProfileCount: 762670,
    workloadProfiles: [],
  },
  resourceType: "azure_container_apps_environment",
};
```

### `models.ResourceHeartbeatDataAzureServiceBusNamespace`

```typescript
const value: models.ResourceHeartbeatDataAzureServiceBusNamespace = {
  data: {
    events: [],
    name: "<value>",
    privateEndpointConnectionCount: 297212,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "updating",
      partial: false,
      stale: false,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```

