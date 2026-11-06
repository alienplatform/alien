# AwsMetricSource

## Example Usage

```typescript
import { AwsMetricSource } from "@alienplatform/manager-api/models";

let value: AwsMetricSource = {
  dimensions: {
    "key": "<value>",
    "key1": "<value>",
  },
  metricName: "<value>",
  namespace: "<value>",
  periodSeconds: 133886,
  statistic: "<value>",
  unit: "degree Celsius",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `dimensions`                                                               | Record<string, *string*>                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `metricName`                                                               | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `monotonic`                                                                | *boolean*                                                                  | :heavy_minus_sign:                                                         | True only for non-negative count/byte totals; arbitrary sums may decrease. |
| `namespace`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `periodSeconds`                                                            | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `statistic`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `unit`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
