# MetricsSourceAws

## Example Usage

```typescript
import { MetricsSourceAws } from "@alienplatform/manager-api/models";

let value: MetricsSourceAws = {
  config: [
    {
      dimensions: {},
      metricName: "<value>",
      namespace: "<value>",
      periodSeconds: 767028,
      statistic: "<value>",
      unit: "kilogram",
    },
  ],
  type: "aws",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `config`                                                 | [models.AwsMetricSource](../models/awsmetricsource.md)[] | :heavy_check_mark:                                       | N/A                                                      |
| `type`                                                   | *"aws"*                                                  | :heavy_check_mark:                                       | N/A                                                      |
