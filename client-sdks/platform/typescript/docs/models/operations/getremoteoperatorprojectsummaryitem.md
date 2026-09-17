# GetRemoteOperatorProjectSummaryItem

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryItem } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryItem = {
  id: "<id>",
  name: "<value>",
  deploymentGroupId: "<id>",
  state: "connecting",
  stateReason: "<value>",
  status: "<value>",
  lastHeartbeatAt: new Date("2024-12-04T02:09:44.054Z"),
  lastSuccessfulDiagnosticAt: new Date("2026-12-09T14:53:16.456Z"),
  operatorVersion: "<value>",
  operatorScope: "<value>",
  operatorPermission: "<value>",
  expectedImage: {
    source: "package",
    packageId: "<id>",
    packageVersion: "<value>",
    image: "https://picsum.photos/seed/z2Vxf/3958/3068",
    digest: "<value>",
    renderedAt: new Date("2024-01-14T18:52:05.063Z"),
  },
  runningImage: {
    source: "configured",
    packageId: "<id>",
    packageVersion: "<value>",
    image: "https://loremflickr.com/805/2403?lock=248237760037676",
    digest: "<value>",
    observedAt: new Date("2026-11-28T23:12:21.731Z"),
  },
  imageStatus: "unconfigured",
  operationSync: {
    status: "stuck",
    targetBundleHash: "<value>",
    observedBundleHash: "<value>",
    targetSetAt: new Date("2026-02-07T00:59:44.962Z"),
    observedAt: new Date("2026-08-26T04:48:28.141Z"),
    error: "<value>",
  },
};
```

## Fields

| Field                                                                                             | Type                                                                                              | Required                                                                                          | Description                                                                                       |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `id`                                                                                              | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `name`                                                                                            | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `deploymentGroupId`                                                                               | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `state`                                                                                           | [operations.ItemState](../../models/operations/itemstate.md)                                      | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `stateReason`                                                                                     | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `status`                                                                                          | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `lastHeartbeatAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)     | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `lastSuccessfulDiagnosticAt`                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)     | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `operatorVersion`                                                                                 | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `operatorScope`                                                                                   | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `operatorPermission`                                                                              | *string*                                                                                          | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `expectedImage`                                                                                   | [models.RemoteOperatorInstallReceipt](../../models/remoteoperatorinstallreceipt.md)               | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `runningImage`                                                                                    | [models.ObservedRemoteOperatorImageIdentity](../../models/observedremoteoperatorimageidentity.md) | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `imageStatus`                                                                                     | [operations.ImageStatus](../../models/operations/imagestatus.md)                                  | :heavy_check_mark:                                                                                | N/A                                                                                               |
| `operationSync`                                                                                   | [operations.OperationSync](../../models/operations/operationsync.md)                              | :heavy_check_mark:                                                                                | N/A                                                                                               |