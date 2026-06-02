# ImportDeploymentRequest

Request schema for importing a deployment from resolved setup infrastructure


## Supported Types

### `models.ForwardImportRequest`

```typescript
const value: models.ForwardImportRequest = {
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  source: {
    deploymentName: "<value>",
    resourcePrefix: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    platform: "kubernetes",
    region: "<value>",
    setupTarget: "<value>",
    setupImportFormatVersion: 171752,
    setupFingerprint: "<value>",
    setupFingerprintVersion: 188662,
    stackSettings: {},
    resources: [
      {
        id: "<id>",
        type: "<value>",
        importData: {
          "key": "<value>",
          "key1": "<value>",
          "key2": "<value>",
        },
      },
    ],
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
  runtimeMetadata: {},
  deploymentProtocolVersion: 75885,
  currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  setupTarget: "<value>",
  setupFingerprint: "<value>",
  setupFingerprintVersion: 119877,
};
```

