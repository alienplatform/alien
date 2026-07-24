# ComputePoolSelectionFixed

Fixed number of machines.

## Example Usage

```typescript
import { ComputePoolSelectionFixed } from "@alienplatform/manager-api/models";

let value: ComputePoolSelectionFixed = {
  machines: 5255,
  mode: "fixed",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `failureDomains`                                                     | [models.FailureDomainSelection](../models/failuredomainselection.md) | :heavy_minus_sign:                                                   | N/A                                                                  |
| `machine`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | Provider machine type selected for this deployment.                  |
| `machines`                                                           | *number*                                                             | :heavy_check_mark:                                                   | Number of machines to run.                                           |
| `mode`                                                               | *"fixed"*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |