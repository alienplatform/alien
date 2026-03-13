# SyncAcquireResponseEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { SyncAcquireResponseEnvironmentInfoAws } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `accountId`                                                                                        | *string*                                                                                           | :heavy_check_mark:                                                                                 | AWS account ID                                                                                     |
| `region`                                                                                           | *string*                                                                                           | :heavy_check_mark:                                                                                 | AWS region                                                                                         |
| `platform`                                                                                         | [models.SyncAcquireResponseCurrentPlatformAws](../models/syncacquireresponsecurrentplatformaws.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |