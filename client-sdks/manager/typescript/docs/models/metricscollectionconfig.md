# MetricsCollectionConfig

## Example Usage

```typescript
import { MetricsCollectionConfig } from "@alienplatform/manager-api/models";

let value: MetricsCollectionConfig = {
  intervalSeconds: 426430,
  sourceLagSeconds: 152581,
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `builtInResources`                                                                     | *boolean*                                                                              | :heavy_minus_sign:                                                                     | Derive sources only from trusted deployed resource outputs; never grants IAM rights.   |
| `intervalSeconds`                                                                      | *number*                                                                               | :heavy_check_mark:                                                                     | Poll at least this many seconds apart (minimum 60).                                    |
| `jobs`                                                                                 | [models.MetricsCollectionJob](../models/metricscollectionjob.md)[]                     | :heavy_minus_sign:                                                                     | Explicit sources. Configuring sources never adds IAM permissions.                      |
| `sourceLagSeconds`                                                                     | *number*                                                                               | :heavy_check_mark:                                                                     | Wait for provider emission before closing an interval (at least 60 for cloud sources). |
