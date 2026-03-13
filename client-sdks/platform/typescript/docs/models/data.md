# Data

Represents all possible events in the Alien system


## Supported Types

### `models.DataLoadingConfiguration`

```typescript
const value: models.DataLoadingConfiguration = {
  type: "LoadingConfiguration",
};
```

### `models.DataFinished`

```typescript
const value: models.DataFinished = {
  type: "Finished",
};
```

### `models.DataBuildingStack`

```typescript
const value: models.DataBuildingStack = {
  stack: "<value>",
  type: "BuildingStack",
};
```

### `models.DataRunningPreflights`

```typescript
const value: models.DataRunningPreflights = {
  platform: "<value>",
  stack: "<value>",
  type: "RunningPreflights",
};
```

### `models.DataDownloadingAlienRuntime`

```typescript
const value: models.DataDownloadingAlienRuntime = {
  targetTriple: "<value>",
  type: "DownloadingAlienRuntime",
  url: "https://dim-jellyfish.com/",
};
```

### `models.DataBuildingResource`

```typescript
const value: models.DataBuildingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "BuildingResource",
};
```

### `models.DataBuildingImage`

```typescript
const value: models.DataBuildingImage = {
  image: "https://loremflickr.com/965/1538?lock=536245262441792",
  type: "BuildingImage",
};
```

### `models.DataPushingImage`

```typescript
const value: models.DataPushingImage = {
  image: "https://picsum.photos/seed/bgd6b4HoNE/948/3236",
  type: "PushingImage",
};
```

### `models.DataPushingStack`

```typescript
const value: models.DataPushingStack = {
  platform: "<value>",
  stack: "<value>",
  type: "PushingStack",
};
```

### `models.DataPushingResource`

```typescript
const value: models.DataPushingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "PushingResource",
};
```

### `models.DataCreatingRelease`

```typescript
const value: models.DataCreatingRelease = {
  project: "<value>",
  type: "CreatingRelease",
};
```

### `models.DataCompilingCode`

```typescript
const value: models.DataCompilingCode = {
  language: "<value>",
  type: "CompilingCode",
};
```

### `models.DataStackStep`

```typescript
const value: models.DataStackStep = {
  nextState: {
    platform: "test",
    resourcePrefix: "<value>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "<value>",
        },
        status: "running",
        type: "<value>",
      },
    },
  },
  type: "StackStep",
};
```

### `models.DataGeneratingCloudFormationTemplate`

```typescript
const value: models.DataGeneratingCloudFormationTemplate = {
  type: "GeneratingCloudFormationTemplate",
};
```

### `models.DataGeneratingTemplate`

```typescript
const value: models.DataGeneratingTemplate = {
  platform: "<value>",
  type: "GeneratingTemplate",
};
```

### `models.DataProvisioningAgent`

```typescript
const value: models.DataProvisioningAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "ProvisioningAgent",
};
```

### `models.DataUpdatingAgent`

```typescript
const value: models.DataUpdatingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "UpdatingAgent",
};
```

### `models.DataDeletingAgent`

```typescript
const value: models.DataDeletingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "DeletingAgent",
};
```

### `models.DataDebuggingAgent`

```typescript
const value: models.DataDebuggingAgent = {
  agentId: "<id>",
  debugSessionId: "<id>",
  type: "DebuggingAgent",
};
```

### `models.DataPreparingEnvironment`

```typescript
const value: models.DataPreparingEnvironment = {
  strategyName: "<value>",
  type: "PreparingEnvironment",
};
```

### `models.DataDeployingStack`

```typescript
const value: models.DataDeployingStack = {
  stackName: "<value>",
  type: "DeployingStack",
};
```

### `models.DataRunningTestFunction`

```typescript
const value: models.DataRunningTestFunction = {
  stackName: "<value>",
  type: "RunningTestFunction",
};
```

### `models.DataCleaningUpStack`

```typescript
const value: models.DataCleaningUpStack = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpStack",
};
```

### `models.DataCleaningUpEnvironment`

```typescript
const value: models.DataCleaningUpEnvironment = {
  stackName: "<value>",
  strategyName: "<value>",
  type: "CleaningUpEnvironment",
};
```

### `models.DataSettingUpPlatformContext`

```typescript
const value: models.DataSettingUpPlatformContext = {
  platformName: "<value>",
  type: "SettingUpPlatformContext",
};
```

### `models.DataEnsuringDockerRepository`

```typescript
const value: models.DataEnsuringDockerRepository = {
  repositoryName: "<value>",
  type: "EnsuringDockerRepository",
};
```

### `models.DataDeployingCloudFormationStack`

```typescript
const value: models.DataDeployingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeployingCloudFormationStack",
};
```

### `models.DataAssumingRole`

```typescript
const value: models.DataAssumingRole = {
  roleArn: "<value>",
  type: "AssumingRole",
};
```

### `models.DataImportingStackStateFromCloudFormation`

```typescript
const value: models.DataImportingStackStateFromCloudFormation = {
  cfnStackName: "<value>",
  type: "ImportingStackStateFromCloudFormation",
};
```

### `models.DataDeletingCloudFormationStack`

```typescript
const value: models.DataDeletingCloudFormationStack = {
  cfnStackName: "<value>",
  currentStatus: "<value>",
  type: "DeletingCloudFormationStack",
};
```

### `models.DataEmptyingBuckets`

```typescript
const value: models.DataEmptyingBuckets = {
  bucketNames: [
    "<value 1>",
  ],
  type: "EmptyingBuckets",
};
```

