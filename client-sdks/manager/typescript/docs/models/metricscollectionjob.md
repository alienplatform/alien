# MetricsCollectionJob

## Example Usage

```typescript
import { MetricsCollectionJob } from "@alienplatform/manager-api/models";

let value: MetricsCollectionJob = {
  id: "<id>",
  resourceId: "<id>",
  source: {
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
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `id`                                                                                   | *string*                                                                               | :heavy_check_mark:                                                                     | Stable job ID; source changes automatically invalidate its prior checkpoint.           |
| `resourceId`                                                                           | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `source`                                                                               | *models.MetricsSource*                                                                 | :heavy_check_mark:                                                                     | An explicitly approved collection source. Provider IAM authorization remains separate. |
