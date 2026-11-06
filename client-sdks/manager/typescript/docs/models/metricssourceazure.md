# MetricsSourceAzure

## Example Usage

```typescript
import { MetricsSourceAzure } from "@alienplatform/manager-api/models";

let value: MetricsSourceAzure = {
  config: {
    aggregation: "<value>",
    intervalSeconds: 644918,
    metricNames: [
      "<value 1>",
      "<value 2>",
    ],
    metricNamespace: "<value>",
    resourceUri: "https://unrealistic-discourse.biz/",
  },
  type: "azure",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `config`                                                   | [models.AzureMetricSource](../models/azuremetricsource.md) | :heavy_check_mark:                                         | N/A                                                        |
| `type`                                                     | *"azure"*                                                  | :heavy_check_mark:                                         | N/A                                                        |
