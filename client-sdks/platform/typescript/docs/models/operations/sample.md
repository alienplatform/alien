# Sample

## Example Usage

```typescript
import { Sample } from "@alienplatform/platform-api/models/operations";

let value: Sample = {
  observedAt: "<value>",
  sourceAt: "<value>",
  metrics: null,
  payload: "<value>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `observedAt`                                                              | *string*                                                                  | :heavy_check_mark:                                                        | N/A                                                                       |
| `sourceAt`                                                                | *string*                                                                  | :heavy_check_mark:                                                        | When Horizon last updated the metrics; null when none were reported.      |
| `metrics`                                                                 | [operations.Metrics](../../models/operations/metrics.md)                  | :heavy_check_mark:                                                        | Null when the replica reported no metrics newer than the previous sample. |
| `payload`                                                                 | *string*                                                                  | :heavy_check_mark:                                                        | The replica as Horizon reported it, as JSON.                              |