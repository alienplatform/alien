# DeploymentDetailResponseEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { DeploymentDetailResponseEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `accountId`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | AWS account ID                                                                                 |
| `region`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | AWS region                                                                                     |
| `platform`                                                                                     | [models.DeploymentDetailResponsePlatformAws](../models/deploymentdetailresponseplatformaws.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |