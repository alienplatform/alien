# SyncAcquireResponseDeploymentPoolsFixed

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPoolsFixed } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPoolsFixed = {
  machines: 634700,
  mode: "fixed",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `failureDomains`                                           | *models.SyncAcquireResponseDeploymentFailureDomainsUnion1* | :heavy_minus_sign:                                         | N/A                                                        |
| `machine`                                                  | *string*                                                   | :heavy_minus_sign:                                         | Provider machine type selected for this deployment.        |
| `machines`                                                 | *number*                                                   | :heavy_check_mark:                                         | Number of machines to run.                                 |
| `mode`                                                     | *"fixed"*                                                  | :heavy_check_mark:                                         | N/A                                                        |
