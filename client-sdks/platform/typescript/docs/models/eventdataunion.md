# EventDataUnion


## Supported Types

### `models.EventDataLoadingConfiguration`

```typescript
const value: models.EventDataLoadingConfiguration = {
  type: "LoadingConfiguration",
};
```

### `models.EventDataFinished`

```typescript
const value: models.EventDataFinished = {
  type: "Finished",
};
```

### `models.EventDataBuildingStack`

```typescript
const value: models.EventDataBuildingStack = {
  stack: "<value>",
  type: "BuildingStack",
};
```

### `models.EventDataRunningPreflights`

```typescript
const value: models.EventDataRunningPreflights = {
  platform: "<value>",
  stack: "<value>",
  type: "RunningPreflights",
};
```

### `models.EventDataDownloadingAlienRuntime`

```typescript
const value: models.EventDataDownloadingAlienRuntime = {
  targetTriple: "<value>",
  type: "DownloadingAlienRuntime",
  url: "https://unlawful-skyline.com",
};
```

### `models.EventDataBuildingResource`

```typescript
const value: models.EventDataBuildingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "BuildingResource",
};
```

### `models.EventDataBuildingImage`

```typescript
const value: models.EventDataBuildingImage = {
  image: "https://loremflickr.com/1460/1419?lock=7755903317896576",
  type: "BuildingImage",
};
```

### `models.EventDataPushingImage`

```typescript
const value: models.EventDataPushingImage = {
  image: "https://picsum.photos/seed/3ETRvL33XQ/3838/1575",
  type: "PushingImage",
};
```

### `models.EventDataPushingStack`

```typescript
const value: models.EventDataPushingStack = {
  platform: "<value>",
  stack: "<value>",
  type: "PushingStack",
};
```

### `models.EventDataPushingResource`

```typescript
const value: models.EventDataPushingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "PushingResource",
};
```

### `models.EventDataCreatingRelease`

```typescript
const value: models.EventDataCreatingRelease = {
  project: "<value>",
  type: "CreatingRelease",
};
```

### `models.EventDataCompilingCode`

```typescript
const value: models.EventDataCompilingCode = {
  language: "<value>",
  type: "CompilingCode",
};
```

### `models.EventDataStackStep`

```typescript
const value: models.EventDataStackStep = {
  nextState: {
    platform: "kubernetes",
    resourcePrefix: "<value>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "<value>",
        },
        status: "pending",
        type: "<value>",
      },
    },
  },
  type: "StackStep",
};
```

### `models.EventDataGeneratingCloudFormationTemplate`

```typescript
const value: models.EventDataGeneratingCloudFormationTemplate = {
  type: "GeneratingCloudFormationTemplate",
};
```

### `models.EventDataGeneratingTemplate`

```typescript
const value: models.EventDataGeneratingTemplate = {
  platform: "<value>",
  type: "GeneratingTemplate",
};
```

### `models.EventDataProvisioningAgent`

```typescript
const value: models.EventDataProvisioningAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "ProvisioningAgent",
};
```

### `models.EventDataUpdatingAgent`

```typescript
const value: models.EventDataUpdatingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "UpdatingAgent",
};
```

### `models.EventDataDeletingAgent`

```typescript
const value: models.EventDataDeletingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "DeletingAgent",
};
```

### `models.EventDataDebuggingAgent`

```typescript
const value: models.EventDataDebuggingAgent = {
  agentId: "<id>",
  debugSessionId: "<id>",
  type: "DebuggingAgent",
};
```

### `models.EventDataPreparingEnvironment`

```typescript
const value: models.EventDataPreparingEnvironment = {
  strategyName: "<value>",
  type: "PreparingEnvironment",
};
```

### `models.EventDataDeployingStack`

```typescript
const value: models.EventDataDeployingStack = {
  stackName: "<value>",
  type: "DeployingStack",
};
```

### `models.EventDataRunningTestWorker`

```typescript
const value: models.EventDataRunningTestWorker = {
  stackName: "<value>",
  type: "RunningTestWorker",
};
```

### `models.EventDataCleaningUpStack`

```typescript
const value: models.EventDataCleaningUpStack = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpStack",
};
```

### `models.EventDataCleaningUpEnvironment`

```typescript
const value: models.EventDataCleaningUpEnvironment = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpEnvironment",
};
```

### `models.EventDataSettingUpPlatformContext`

```typescript
const value: models.EventDataSettingUpPlatformContext = {
  platformName: "<value>",
  type: "SettingUpPlatformContext",
};
```

### `models.EventDataEnsuringDockerRepository`

```typescript
const value: models.EventDataEnsuringDockerRepository = {
  repositoryName: "<value>",
  type: "EnsuringDockerRepository",
};
```

### `models.EventDataDeployingCloudFormationStack`

```typescript
const value: models.EventDataDeployingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeployingCloudFormationStack",
};
```

### `models.EventDataAssumingRole`

```typescript
const value: models.EventDataAssumingRole = {
  roleArn: "<value>",
  type: "AssumingRole",
};
```

### `models.EventDataImportingStackStateFromCloudFormation`

```typescript
const value: models.EventDataImportingStackStateFromCloudFormation = {
  cfnStackName: "<value>",
  type: "ImportingStackStateFromCloudFormation",
};
```

### `models.EventDataDeletingCloudFormationStack`

```typescript
const value: models.EventDataDeletingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeletingCloudFormationStack",
};
```

### `models.EventDataEmptyingBuckets`

```typescript
const value: models.EventDataEmptyingBuckets = {
  bucketNames: [],
  type: "EmptyingBuckets",
};
```

### `models.EventDataDeploymentCreated`

```typescript
const value: models.EventDataDeploymentCreated = {
  deploymentGroupId: "<id>",
  deploymentId: "<id>",
  type: "DeploymentCreated",
};
```

### `models.EventDataDeploymentReleased`

```typescript
const value: models.EventDataDeploymentReleased = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentReleased",
};
```

### `models.EventDataDeploymentFailed`

```typescript
const value: models.EventDataDeploymentFailed = {
  deploymentId: "<id>",
  error: {
    code: "<value>",
    internal: true,
    message: "<value>",
  },
  phase: "deleting",
  type: "DeploymentFailed",
};
```

### `models.EventDataDeploymentDegraded`

```typescript
const value: models.EventDataDeploymentDegraded = {
  deploymentId: "<id>",
  error: {
    code: "<value>",
    internal: true,
    message: "<value>",
  },
  type: "DeploymentDegraded",
};
```

### `models.EventDataDeploymentRecovered`

```typescript
const value: models.EventDataDeploymentRecovered = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentRecovered",
};
```

### `models.EventDataDeploymentDeleted`

```typescript
const value: models.EventDataDeploymentDeleted = {
  deploymentId: "<id>",
  type: "DeploymentDeleted",
};
```

### `models.EventDataDeploymentRetryRequested`

```typescript
const value: models.EventDataDeploymentRetryRequested = {
  deploymentId: "<id>",
  type: "DeploymentRetryRequested",
};
```

### `models.EventDataDeploymentRedeployRequested`

```typescript
const value: models.EventDataDeploymentRedeployRequested = {
  deploymentId: "<id>",
  releaseId: "<id>",
  type: "DeploymentRedeployRequested",
};
```

### `models.EventDataDeploymentReleasePinned`

```typescript
const value: models.EventDataDeploymentReleasePinned = {
  deploymentId: "<id>",
  pinnedReleaseId: "<id>",
  type: "DeploymentReleasePinned",
};
```

### `models.EventDataDeploymentReleaseUnpinned`

```typescript
const value: models.EventDataDeploymentReleaseUnpinned = {
  deploymentId: "<id>",
  previousPinnedReleaseId: "<id>",
  type: "DeploymentReleaseUnpinned",
};
```

### `models.EventDataDeploymentEnvironmentUpdated`

```typescript
const value: models.EventDataDeploymentEnvironmentUpdated = {
  changedKeys: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  deploymentId: "<id>",
  type: "DeploymentEnvironmentUpdated",
};
```

### `models.EventDataDeploymentDeletionRequested`

```typescript
const value: models.EventDataDeploymentDeletionRequested = {
  deploymentId: "<id>",
  type: "DeploymentDeletionRequested",
};
```
