# SyncAcquireResponseDeploymentEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `accountId`                                                                                                            | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | AWS account ID                                                                                                         |
| `region`                                                                                                               | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | AWS region                                                                                                             |
| `platform`                                                                                                             | [models.SyncAcquireResponseDeploymentCurrentPlatformAws](../models/syncacquireresponsedeploymentcurrentplatformaws.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |