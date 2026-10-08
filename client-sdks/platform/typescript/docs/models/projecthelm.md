# ProjectHelm

Helm chart package configuration. If null, Helm packages will not be generated.

## Example Usage

```typescript
import { ProjectHelm } from "@alienplatform/platform-api/models";

let value: ProjectHelm = {
  chartName: "<value>",
  description: "mid brr qua once yet fully",
  enabled: false,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `chartName`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | Chart name (e.g., "acme-operator")                                         |
| `description`                                                              | *string*                                                                   | :heavy_check_mark:                                                         | Human-friendly description of the chart                                    |
| `logCollector`                                                             | [models.ProjectLogCollector](../models/projectlogcollector.md)             | :heavy_minus_sign:                                                         | Default log collection mode in a generated Helm chart.                     |
| `enabled`                                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | Whether Helm chart package generation is enabled                           |
| `setupResources`                                                           | [models.ProjectSetupResources](../models/projectsetupresources.md)         | :heavy_minus_sign:                                                         | N/A                                                                        |
| `runtimePersistence`                                                       | [models.ProjectRuntimePersistence](../models/projectruntimepersistence.md) | :heavy_minus_sign:                                                         | N/A                                                                        |
