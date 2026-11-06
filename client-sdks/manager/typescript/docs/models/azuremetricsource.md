# AzureMetricSource

## Example Usage

```typescript
import { AzureMetricSource } from "@alienplatform/manager-api/models";

let value: AzureMetricSource = {
  aggregation: "<value>",
  intervalSeconds: 917636,
  metricNames: [],
  metricNamespace: "<value>",
  resourceUri: "https://heartfelt-litter.com/",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `aggregation`                                                            | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `filter`                                                                 | *string*                                                                 | :heavy_minus_sign:                                                       | Explicit dimension filter, for example `EntityName eq 'approved-queue'`. |
| `intervalSeconds`                                                        | *number*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `metricNames`                                                            | *string*[]                                                               | :heavy_check_mark:                                                       | N/A                                                                      |
| `metricNamespace`                                                        | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `monotonic`                                                              | *boolean*                                                                | :heavy_minus_sign:                                                       | True for count/byte totals, false for arbitrary aggregated values.       |
| `resourceUri`                                                            | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
