# UpdateProjectPackagesConfigHelm

Helm chart package configuration. If null, Helm packages will not be generated.

## Example Usage

```typescript
import { UpdateProjectPackagesConfigHelm } from "@alienplatform/platform-api/models";

let value: UpdateProjectPackagesConfigHelm = {
  chartName: "<value>",
  description:
    "second qua wherever tenderly apropos despite seriously whenever digit unusual",
  enabled: true,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `chartName`                                                                                                        | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Chart name (e.g., "acme-operator")                                                                                 |
| `description`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Human-friendly description of the chart                                                                            |
| `logCollector`                                                                                                     | [models.UpdateProjectPackagesConfigLogCollector](../models/updateprojectpackagesconfiglogcollector.md)             | :heavy_minus_sign:                                                                                                 | Default log collection mode in a generated Helm chart.                                                             |
| `enabled`                                                                                                          | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | Whether Helm chart package generation is enabled                                                                   |
| `setupResources`                                                                                                   | [models.UpdateProjectPackagesConfigSetupResources](../models/updateprojectpackagesconfigsetupresources.md)         | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
| `runtimePersistence`                                                                                               | [models.UpdateProjectPackagesConfigRuntimePersistence](../models/updateprojectpackagesconfigruntimepersistence.md) | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |
