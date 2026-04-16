# DeploymentDetailResponseExtendAw

AWS-specific platform permission configuration

## Example Usage

```typescript
import { DeploymentDetailResponseExtendAw } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseExtendAw = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                              | [models.DeploymentDetailResponseExtendAwBinding](../models/deploymentdetailresponseextendawbinding.md) | :heavy_check_mark:                                                                                     | Generic binding configuration for permissions                                                          |
| `grant`                                                                                                | [models.DeploymentDetailResponseExtendAwGrant](../models/deploymentdetailresponseextendawgrant.md)     | :heavy_check_mark:                                                                                     | Grant permissions for a specific cloud platform                                                        |