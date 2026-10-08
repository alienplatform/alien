# AcquireResponse

## Example Usage

```typescript
import { AcquireResponse } from "@alienplatform/manager-api/models";

let value: AcquireResponse = {
  deployments: [],
};
```

## Fields

| Field                                                                                                                     | Type                                                                                                                      | Required                                                                                                                  | Description                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `deployments`                                                                                                             | [models.AcquiredDeploymentResponse](../models/acquireddeploymentresponse.md)[]                                            | :heavy_check_mark:                                                                                                        | N/A                                                                                                                       |
| `notAcquired`                                                                                                             | [models.UnacquiredDeployment](../models/unacquireddeployment.md)[]                                                        | :heavy_minus_sign:                                                                                                        | Bounded reasons for explicitly requested deployments that were not acquired.<br/>Empty for discovery-style batch acquisition. |