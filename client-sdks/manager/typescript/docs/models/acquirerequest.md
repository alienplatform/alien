# AcquireRequest

## Example Usage

```typescript
import { AcquireRequest } from "@alienplatform/manager-api/models";

let value: AcquireRequest = {
  deploymentModel: "push",
  session: "<value>",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `acquireMode`                                                          | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `deploymentIds`                                                        | *string*[]                                                             | :heavy_minus_sign:                                                     | N/A                                                                    |
| `deploymentModel`                                                      | [models.DeploymentModel](../models/deploymentmodel.md)                 | :heavy_check_mark:                                                     | Deployment model: how updates are delivered to the remote environment. |
| `limit`                                                                | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `platforms`                                                            | [models.PlatformEnum](../models/platformenum.md)[]                     | :heavy_minus_sign:                                                     | N/A                                                                    |
| `session`                                                              | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `setupMethod`                                                          | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `statuses`                                                             | *string*[]                                                             | :heavy_minus_sign:                                                     | N/A                                                                    |