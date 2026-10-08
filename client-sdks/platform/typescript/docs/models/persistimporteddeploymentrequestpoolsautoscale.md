# PersistImportedDeploymentRequestPoolsAutoscale

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPoolsAutoscale } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPoolsAutoscale = {
  max: 534859,
  min: 283632,
  mode: "autoscale",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `failureDomains`                                              | *models.PersistImportedDeploymentRequestFailureDomainsUnion2* | :heavy_minus_sign:                                            | N/A                                                           |
| `machine`                                                     | *string*                                                      | :heavy_minus_sign:                                            | Provider machine type selected for this deployment.           |
| `max`                                                         | *number*                                                      | :heavy_check_mark:                                            | Maximum machine count.                                        |
| `min`                                                         | *number*                                                      | :heavy_check_mark:                                            | Minimum machine count.                                        |
| `mode`                                                        | *"autoscale"*                                                 | :heavy_check_mark:                                            | N/A                                                           |
