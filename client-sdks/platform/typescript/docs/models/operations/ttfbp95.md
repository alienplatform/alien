# TtfbP95

Histogram percentile bounds in seconds; null means no completed observations.

## Example Usage

```typescript
import { TtfbP95 } from "@alienplatform/platform-api/models/operations";

let value: TtfbP95 = {
  lower: 0.01,
  upper: 0.025,
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       | Example                                                                           |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `lower`                                                                           | *number*                                                                          | :heavy_check_mark:                                                                | Lower histogram bound in seconds.                                                 | 0.01                                                                              |
| `upper`                                                                           | *number*                                                                          | :heavy_check_mark:                                                                | Upper histogram bound in seconds; null means overflow with no finite upper bound. | 0.025                                                                             |