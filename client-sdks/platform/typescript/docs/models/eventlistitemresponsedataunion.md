# EventListItemResponseDataUnion


## Supported Types

### `models.EventListItemResponseDataLoadingConfiguration`

```typescript
const value: models.EventListItemResponseDataLoadingConfiguration = {
  type: "LoadingConfiguration",
};
```

### `models.EventListItemResponseDataFinished`

```typescript
const value: models.EventListItemResponseDataFinished = {
  type: "Finished",
};
```

### `models.EventListItemResponseDataBuildingStack`

```typescript
const value: models.EventListItemResponseDataBuildingStack = {
  stack: "<value>",
  type: "BuildingStack",
};
```

### `models.EventListItemResponseDataRunningPreflights`

```typescript
const value: models.EventListItemResponseDataRunningPreflights = {
  platform: "<value>",
  stack: "<value>",
  type: "RunningPreflights",
};
```

### `models.EventListItemResponseDataDownloadingAlienRuntime`

```typescript
const value: models.EventListItemResponseDataDownloadingAlienRuntime = {
  targetTriple: "<value>",
  type: "DownloadingAlienRuntime",
  url: "https://hefty-eggplant.net",
};
```

### `models.EventListItemResponseDataBuildingResource`

```typescript
const value: models.EventListItemResponseDataBuildingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "BuildingResource",
};
```

### `models.EventListItemResponseDataBuildingImage`

```typescript
const value: models.EventListItemResponseDataBuildingImage = {
  image: "https://loremflickr.com/3474/373?lock=4089420585640817",
  type: "BuildingImage",
};
```

### `models.EventListItemResponseDataPushingImage`

```typescript
const value: models.EventListItemResponseDataPushingImage = {
  image: "https://picsum.photos/seed/3kNN0/505/3308",
  type: "PushingImage",
};
```

### `models.EventListItemResponseDataPushingStack`

```typescript
const value: models.EventListItemResponseDataPushingStack = {
  platform: "<value>",
  stack: "<value>",
  type: "PushingStack",
};
```

### `models.EventListItemResponseDataPushingResource`

```typescript
const value: models.EventListItemResponseDataPushingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "PushingResource",
};
```

### `models.EventListItemResponseDataCreatingRelease`

```typescript
const value: models.EventListItemResponseDataCreatingRelease = {
  project: "<value>",
  type: "CreatingRelease",
};
```

### `models.EventListItemResponseDataCompilingCode`

```typescript
const value: models.EventListItemResponseDataCompilingCode = {
  language: "<value>",
  type: "CompilingCode",
};
```

### `models.EventListItemResponseDataStackStep`

```typescript
const value: models.EventListItemResponseDataStackStep = {
  nextState: {
    platform: "test",
    resourcePrefix: "<value>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "<value>",
        },
        status: "delete-failed",
        type: "<value>",
      },
    },
  },
  type: "StackStep",
};
```

### `models.EventListItemResponseDataGeneratingCloudFormationTemplate`

```typescript
const value: models.EventListItemResponseDataGeneratingCloudFormationTemplate =
  {
    type: "GeneratingCloudFormationTemplate",
  };
```

### `models.EventListItemResponseDataGeneratingTemplate`

```typescript
const value: models.EventListItemResponseDataGeneratingTemplate = {
  platform: "<value>",
  type: "GeneratingTemplate",
};
```

### `models.EventListItemResponseDataProvisioningAgent`

```typescript
const value: models.EventListItemResponseDataProvisioningAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "ProvisioningAgent",
};
```

### `models.EventListItemResponseDataUpdatingAgent`

```typescript
const value: models.EventListItemResponseDataUpdatingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "UpdatingAgent",
};
```

### `models.EventListItemResponseDataDeletingAgent`

```typescript
const value: models.EventListItemResponseDataDeletingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "DeletingAgent",
};
```

### `models.EventListItemResponseDataDebuggingAgent`

```typescript
const value: models.EventListItemResponseDataDebuggingAgent = {
  agentId: "<id>",
  debugSessionId: "<id>",
  type: "DebuggingAgent",
};
```

### `models.EventListItemResponseDataPreparingEnvironment`

```typescript
const value: models.EventListItemResponseDataPreparingEnvironment = {
  strategyName: "<value>",
  type: "PreparingEnvironment",
};
```

### `models.EventListItemResponseDataDeployingStack`

```typescript
const value: models.EventListItemResponseDataDeployingStack = {
  stackName: "<value>",
  type: "DeployingStack",
};
```

### `models.EventListItemResponseDataRunningTestWorker`

```typescript
const value: models.EventListItemResponseDataRunningTestWorker = {
  stackName: "<value>",
  type: "RunningTestWorker",
};
```

### `models.EventListItemResponseDataCleaningUpStack`

```typescript
const value: models.EventListItemResponseDataCleaningUpStack = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpStack",
};
```

### `models.EventListItemResponseDataCleaningUpEnvironment`

```typescript
const value: models.EventListItemResponseDataCleaningUpEnvironment = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpEnvironment",
};
```

### `models.EventListItemResponseDataSettingUpPlatformContext`

```typescript
const value: models.EventListItemResponseDataSettingUpPlatformContext = {
  platformName: "<value>",
  type: "SettingUpPlatformContext",
};
```

### `models.EventListItemResponseDataEnsuringDockerRepository`

```typescript
const value: models.EventListItemResponseDataEnsuringDockerRepository = {
  repositoryName: "<value>",
  type: "EnsuringDockerRepository",
};
```

### `models.EventListItemResponseDataDeployingCloudFormationStack`

```typescript
const value: models.EventListItemResponseDataDeployingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeployingCloudFormationStack",
};
```

### `models.EventListItemResponseDataAssumingRole`

```typescript
const value: models.EventListItemResponseDataAssumingRole = {
  roleArn: "<value>",
  type: "AssumingRole",
};
```

### `models.EventListItemResponseDataImportingStackStateFromCloudFormation`

```typescript
const value:
  models.EventListItemResponseDataImportingStackStateFromCloudFormation = {
    cfnStackName: "<value>",
    type: "ImportingStackStateFromCloudFormation",
  };
```

### `models.EventListItemResponseDataDeletingCloudFormationStack`

```typescript
const value: models.EventListItemResponseDataDeletingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeletingCloudFormationStack",
};
```

### `models.EventListItemResponseDataEmptyingBuckets`

```typescript
const value: models.EventListItemResponseDataEmptyingBuckets = {
  bucketNames: [],
  type: "EmptyingBuckets",
};
```

### `models.EventListItemResponseDataDeploymentCreated`

```typescript
const value: models.EventListItemResponseDataDeploymentCreated = {
  deploymentGroupId: "<id>",
  deploymentId: "<id>",
  type: "DeploymentCreated",
};
```

### `models.EventListItemResponseDataDeploymentReleased`

```typescript
const value: models.EventListItemResponseDataDeploymentReleased = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentReleased",
};
```

### `models.EventListItemResponseDataDeploymentFailed`

```typescript
const value: models.EventListItemResponseDataDeploymentFailed = {
  deploymentId: "<id>",
  error: {
    code: "<value>",
    internal: false,
    message: "<value>",
  },
  phase: "provisioning",
  type: "DeploymentFailed",
};
```

### `models.EventListItemResponseDataDeploymentDegraded`

```typescript
const value: models.EventListItemResponseDataDeploymentDegraded = {
  deploymentId: "<id>",
  error: {
    code: "<value>",
    internal: false,
    message: "<value>",
  },
  type: "DeploymentDegraded",
};
```

### `models.EventListItemResponseDataDeploymentRecovered`

```typescript
const value: models.EventListItemResponseDataDeploymentRecovered = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentRecovered",
};
```

### `models.EventListItemResponseDataDeploymentDeleted`

```typescript
const value: models.EventListItemResponseDataDeploymentDeleted = {
  deploymentId: "<id>",
  type: "DeploymentDeleted",
};
```

### `models.EventListItemResponseDataDeploymentRetryRequested`

```typescript
const value: models.EventListItemResponseDataDeploymentRetryRequested = {
  deploymentId: "<id>",
  type: "DeploymentRetryRequested",
};
```

### `models.EventListItemResponseDataDeploymentRedeployRequested`

```typescript
const value: models.EventListItemResponseDataDeploymentRedeployRequested = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentRedeployRequested",
};
```

### `models.EventListItemResponseDataDeploymentReleasePinned`

```typescript
const value: models.EventListItemResponseDataDeploymentReleasePinned = {
  deploymentId: "<id>",
  pinnedReleaseId: "<id>",
  type: "DeploymentReleasePinned",
};
```

### `models.EventListItemResponseDataDeploymentReleaseUnpinned`

```typescript
const value: models.EventListItemResponseDataDeploymentReleaseUnpinned = {
  deploymentId: "<id>",
  previousPinnedReleaseId: "<id>",
  type: "DeploymentReleaseUnpinned",
};
```

### `models.EventListItemResponseDataDeploymentEnvironmentUpdated`

```typescript
const value: models.EventListItemResponseDataDeploymentEnvironmentUpdated = {
  changedKeys: [
    "<value 1>",
  ],
  deploymentId: "<id>",
  type: "DeploymentEnvironmentUpdated",
};
```

### `models.EventListItemResponseDataDeploymentDeletionRequested`

```typescript
const value: models.EventListItemResponseDataDeploymentDeletionRequested = {
  deploymentId: "<id>",
  type: "DeploymentDeletionRequested",
};
```
