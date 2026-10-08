# SetupItemStatus

## Example Usage

```typescript
import { SetupItemStatus } from "@alienplatform/platform-api/models";

let value: SetupItemStatus = {
  item: "sandbox",
  source: {
    type: "project-release",
    releaseChannel: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  },
  required: false,
  status: "connected",
  deploymentIds: [],
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `item`                                                                           | [models.SetupItemStatusItem](../models/setupitemstatusitem.md)                   | :heavy_check_mark:                                                               | N/A                                                                              |
| `source`                                                                         | *models.SetupItemStatusSourceUnion*                                              | :heavy_check_mark:                                                               | N/A                                                                              |
| `required`                                                                       | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `configuration`                                                                  | [models.SetupItemStatusConfiguration](../models/setupitemstatusconfiguration.md) | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.SetupItemStatusStatus](../models/setupitemstatusstatus.md)               | :heavy_check_mark:                                                               | N/A                                                                              |
| `deploymentIds`                                                                  | *string*[]                                                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `blocker`                                                                        | [models.SetupItemStatusBlocker](../models/setupitemstatusblocker.md)             | :heavy_minus_sign:                                                               | N/A                                                                              |