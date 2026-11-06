# MetricsSourceGcp

## Example Usage

```typescript
import { MetricsSourceGcp } from "@alienplatform/manager-api/models";

let value: MetricsSourceGcp = {
  config: {
    filter: "<value>",
    projectId: "<id>",
  },
  type: "gcp",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `config`                                               | [models.GcpMetricSource](../models/gcpmetricsource.md) | :heavy_check_mark:                                     | N/A                                                    |
| `type`                                                 | *"gcp"*                                                | :heavy_check_mark:                                     | N/A                                                    |
