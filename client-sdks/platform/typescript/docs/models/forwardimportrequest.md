# ForwardImportRequest

## Example Usage

```typescript
import { ForwardImportRequest } from "@alienplatform/platform-api/models";

let value: ForwardImportRequest = {
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  source: {
    deploymentName: "<value>",
    stackPrefix: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    platform: "kubernetes",
    region: "<value>",
    setupTarget: "<value>",
    setupFingerprint: "<value>",
    setupFingerprintVersion: 171752,
    stackSettings: {},
    managementConfig: {
      managingRoleArn: "<value>",
      platform: "aws",
    },
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

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `mode`                                                                                        | [models.ForwardImportRequestMode](../models/forwardimportrequestmode.md)                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `project`                                                                                     | *string*                                                                                      | :heavy_minus_sign:                                                                            | Project ID or name. Required for user-session callers.                                        |                                                                                               |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_minus_sign:                                                                            | Required for user-session callers. Deployment-group tokens use their own group automatically. | dg_r27ict8c7vcgsumpj90ackf7b                                                                  |
| `managerId`                                                                                   | *string*                                                                                      | :heavy_minus_sign:                                                                            | Manager ID. If omitted, the first suitable manager for the source platform is used.           | mgr_enxscjrqiiu2lrc672hwwuc5                                                                  |
| `source`                                                                                      | [models.ImportSource](../models/importsource.md)                                              | :heavy_check_mark:                                                                            | Resolved setup import payload                                                                 |                                                                                               |