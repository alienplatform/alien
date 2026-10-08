# PersistImportedDeploymentRequestPoolsFixed

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPoolsFixed } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPoolsFixed = {
  machines: 521550,
  mode: "fixed",
};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `failureDomains`                                              | *models.PersistImportedDeploymentRequestFailureDomainsUnion1* | :heavy_minus_sign:                                            | N/A                                                           |
| `machine`                                                     | *string*                                                      | :heavy_minus_sign:                                            | Provider machine type selected for this deployment.           |
| `machines`                                                    | *number*                                                      | :heavy_check_mark:                                            | Number of machines to run.                                    |
| `mode`                                                        | *"fixed"*                                                     | :heavy_check_mark:                                            | N/A                                                           |
