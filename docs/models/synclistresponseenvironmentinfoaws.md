# SyncListResponseEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { SyncListResponseEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: SyncListResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `accountId`                                                                                                  | *string*                                                                                                     | :heavy_check_mark:                                                                                           | AWS account ID                                                                                               |
| `region`                                                                                                     | *string*                                                                                                     | :heavy_check_mark:                                                                                           | AWS region                                                                                                   |
| `platform`                                                                                                   | [models.SyncListResponseEnvironmentInfoPlatformAws](../models/synclistresponseenvironmentinfoplatformaws.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |