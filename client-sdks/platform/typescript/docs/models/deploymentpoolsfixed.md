# DeploymentPoolsFixed

## Example Usage

```typescript
import { DeploymentPoolsFixed } from "@alienplatform/platform-api/models";

let value: DeploymentPoolsFixed = {
  machines: 563700,
  mode: "fixed",
};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `dataDiskGib`                                                                                                                              | *number*                                                                                                                                   | :heavy_minus_sign:                                                                                                                         | Data disk size per machine, in whole GiB, within the pool's declared<br/>range. Omitted uses the release default; 0 runs the pool without one. |
| `failureDomains`                                                                                                                           | *models.DeploymentFailureDomainsUnion1*                                                                                                    | :heavy_minus_sign:                                                                                                                         | N/A                                                                                                                                        |
| `machine`                                                                                                                                  | *string*                                                                                                                                   | :heavy_minus_sign:                                                                                                                         | Provider machine type selected for this deployment.                                                                                        |
| `machines`                                                                                                                                 | *number*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Number of machines to run.                                                                                                                 |
| `mode`                                                                                                                                     | *"fixed"*                                                                                                                                  | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |