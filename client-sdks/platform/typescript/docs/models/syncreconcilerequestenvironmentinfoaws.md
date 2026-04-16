# SyncReconcileRequestEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { SyncReconcileRequestEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `accountId`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | AWS account ID                                                                         |
| `region`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | AWS region                                                                             |
| `platform`                                                                             | [models.SyncReconcileRequestPlatformAws](../models/syncreconcilerequestplatformaws.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |