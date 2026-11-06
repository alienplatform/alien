# GcpMetricSource

## Example Usage

```typescript
import { GcpMetricSource } from "@alienplatform/manager-api/models";

let value: GcpMetricSource = {
  filter: "<value>",
  projectId: "<id>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `filter`                                                                  | *string*                                                                  | :heavy_check_mark:                                                        | N/A                                                                       |
| `minimumLagSeconds`                                                       | *number*                                                                  | :heavy_minus_sign:                                                        | Provider visibility delay. This is independent of the collection cadence. |
| `monotonic`                                                               | *boolean*                                                                 | :heavy_minus_sign:                                                        | Whether numeric DELTA points are known non-negative counter increments.   |
| `projectId`                                                               | *string*                                                                  | :heavy_check_mark:                                                        | N/A                                                                       |
