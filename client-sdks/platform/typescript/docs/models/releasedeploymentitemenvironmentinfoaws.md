# ReleaseDeploymentItemEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { ReleaseDeploymentItemEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentItemEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `accountId`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | AWS account ID                                                                           |
| `region`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | AWS region                                                                               |
| `platform`                                                                               | [models.ReleaseDeploymentItemPlatformAws](../models/releasedeploymentitemplatformaws.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
