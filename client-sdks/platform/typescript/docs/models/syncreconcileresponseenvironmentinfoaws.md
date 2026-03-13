# SyncReconcileResponseEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { SyncReconcileResponseEnvironmentInfoAws } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `accountId`                                                                                            | *string*                                                                                               | :heavy_check_mark:                                                                                     | AWS account ID                                                                                         |
| `region`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | AWS region                                                                                             |
| `platform`                                                                                             | [models.SyncReconcileResponseCurrentPlatformAws](../models/syncreconcileresponsecurrentplatformaws.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |