# PlanDeploymentComputePoolsFixed

## Example Usage

```typescript
import { PlanDeploymentComputePoolsFixed } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputePoolsFixed = {
  machines: 8761,
  mode: "fixed",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `failureDomains`                                       | *operations.PlanDeploymentComputeFailureDomainsUnion1* | :heavy_minus_sign:                                     | N/A                                                    |
| `machine`                                              | *string*                                               | :heavy_minus_sign:                                     | Provider machine type selected for this deployment.    |
| `machines`                                             | *number*                                               | :heavy_check_mark:                                     | Number of machines to run.                             |
| `mode`                                                 | *"fixed"*                                              | :heavy_check_mark:                                     | N/A                                                    |