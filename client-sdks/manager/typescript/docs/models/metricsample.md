# MetricSample

## Example Usage

```typescript
import { MetricSample } from "@alienplatform/manager-api/models";

let value: MetricSample = {
  unit: "milliseconds",
  value: 5632.01,
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `unit`                                       | [models.MetricUnit](../models/metricunit.md) | :heavy_check_mark:                           | N/A                                          |
| `value`                                      | *number*                                     | :heavy_check_mark:                           | N/A                                          |