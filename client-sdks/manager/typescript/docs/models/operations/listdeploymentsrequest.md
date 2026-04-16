# ListDeploymentsRequest

## Example Usage

```typescript
import { ListDeploymentsRequest } from "@alienplatform/manager-api/models/operations";

let value: ListDeploymentsRequest = {};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `deploymentGroupId`                              | *string*                                         | :heavy_minus_sign:                               | Filter by deployment group ID                    |
| `include`                                        | *string*[]                                       | :heavy_minus_sign:                               | Include related resources (e.g. deploymentGroup) |