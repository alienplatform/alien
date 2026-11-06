# ImportDeploymentRequest

Request schema for importing a deployment from resolved setup infrastructure


## Supported Types

### `models.ForwardImportRequest`

```typescript
const value: models.ForwardImportRequest = {
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  source: {
    deploymentName: "<value>",
    resourcePrefix: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    platform: "machines",
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
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  platform: "kubernetes",
  stackSettings: {},
  runtimeMetadata: {},
  deploymentProtocolVersion: 75885,
  currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  desiredReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  setupTarget: "<value>",
  setupFingerprint: "<value>",
  setupFingerprintVersion: 119877,
};
```
