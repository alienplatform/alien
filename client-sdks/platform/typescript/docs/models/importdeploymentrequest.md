# ImportDeploymentRequest

Request schema for importing a deployment from resolved distribution infrastructure


## Supported Types

### `models.ForwardImportRequest`

```typescript
const value: models.ForwardImportRequest = {
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  source: {
    deploymentName: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    platform: "kubernetes",
    region: "<value>",
    stackSettings: {},
    managementConfig: {
      managingRoleArn: "<value>",
      platform: "aws",
    },
    resources: [],
  },
};
```

### `models.PersistImportedDeploymentRequest`

```typescript
const value: models.PersistImportedDeploymentRequest = {
  mode: "persist",
  name: "<value>",
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  platform: "kubernetes",
  stackSettings: {},
  currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

