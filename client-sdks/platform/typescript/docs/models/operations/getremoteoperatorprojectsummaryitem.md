# GetRemoteOperatorProjectSummaryItem

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryItem } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryItem = {
  id: "<id>",
  name: "<value>",
  deploymentGroupId: "<id>",
  platform: "ecs",
  region: "<value>",
  state: "stale",
  stateReason: "<value>",
  status: "<value>",
  lastHeartbeatAt: new Date("2026-12-09T14:53:16.456Z"),
  lastSuccessfulDiagnosticAt: new Date("2026-08-22T08:34:06.872Z"),
  operatorVersion: "<value>",
  operatorScope: "<value>",
  operatorPermission: "<value>",
  expectedImage: {
    source: "configured",
    packageId: "<id>",
    packageVersion: "<value>",
    image: "https://picsum.photos/seed/Vxf0k/3152/375",
    digest: "<value>",
    renderedAt: new Date("2025-09-08T21:19:44.313Z"),
  },
  previousExpectedImage: {
    source: "package",
    packageId: "<id>",
    packageVersion: "<value>",
    image: "https://picsum.photos/seed/EMoHCSfyM/1758/111",
    digest: "<value>",
    renderedAt: new Date("2026-12-27T19:53:42.281Z"),
  },
  runningImage: {
    source: "package",
    packageId: "<id>",
    packageVersion: null,
    image: "https://loremflickr.com/1932/483?lock=2165930466772595",
    digest: "<value>",
    observedAt: new Date("2026-03-04T09:53:52.702Z"),
  },
  imageStatus: "drifted",
  operationSync: {
    status: "stuck",
    targetBundleHash: "<value>",
    observedBundleHash: "<value>",
    targetSetAt: new Date("2025-01-05T07:00:11.339Z"),
    observedAt: new Date("2026-05-03T15:45:19.418Z"),
    error: "<value>",
  },
  application: {
    state: "no-desired",
    reason: "<value>",
    reported: {
      releaseId: "<id>",
      version: "<value>",
    },
    desired: {
      releaseId: "<id>",
      version: "<value>",
      source: "pin",
      helmChart: {
        chart: "<value>",
        version: "<value>",
      },
      images: [],
    },
    releaseChannel: "<value>",
    rollback: {
      releaseId: "<id>",
      version: "<value>",
    },
  },
  permissions: {
    status: "outdated",
    recordedAt: new Date("2025-01-20T15:52:35.458Z"),
    reason: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                                               | Type                                                                                                                                                                | Required                                                                                                                                                            | Description                                                                                                                                                         |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                                | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `name`                                                                                                                                                              | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `deploymentGroupId`                                                                                                                                                 | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `platform`                                                                                                                                                          | [operations.GetRemoteOperatorProjectSummaryPlatform](../../models/operations/getremoteoperatorprojectsummaryplatform.md)                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `region`                                                                                                                                                            | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `state`                                                                                                                                                             | [operations.ItemState](../../models/operations/itemstate.md)                                                                                                        | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `stateReason`                                                                                                                                                       | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `status`                                                                                                                                                            | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `lastHeartbeatAt`                                                                                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                                       | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `lastSuccessfulDiagnosticAt`                                                                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                                       | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `operatorVersion`                                                                                                                                                   | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `operatorScope`                                                                                                                                                     | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `operatorPermission`                                                                                                                                                | *string*                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `expectedImage`                                                                                                                                                     | [models.RemoteOperatorInstallReceipt](../../models/remoteoperatorinstallreceipt.md)                                                                                 | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `previousExpectedImage`                                                                                                                                             | [models.RemoteOperatorInstallReceipt](../../models/remoteoperatorinstallreceipt.md)                                                                                 | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `runningImage`                                                                                                                                                      | [models.ObservedRemoteOperatorImageIdentity](../../models/observedremoteoperatorimageidentity.md)                                                                   | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `imageStatus`                                                                                                                                                       | [operations.ImageStatus](../../models/operations/imagestatus.md)                                                                                                    | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `operationSync`                                                                                                                                                     | [operations.OperationSync](../../models/operations/operationsync.md)                                                                                                | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `application`                                                                                                                                                       | [operations.Application](../../models/operations/application.md)                                                                                                    | :heavy_check_mark:                                                                                                                                                  | N/A                                                                                                                                                                 |
| `permissions`                                                                                                                                                       | [operations.Permissions](../../models/operations/permissions.md)                                                                                                    | :heavy_check_mark:                                                                                                                                                  | Installed cloud and Kubernetes permissions. Setup applies them, so enabling or disabling operations changes them only after the installation's setup is re-applied. |