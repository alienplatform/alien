# PackagesConfigPatchHelm

## Example Usage

```typescript
import { PackagesConfigPatchHelm } from "@alienplatform/platform-api/models";

let value: PackagesConfigPatchHelm = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `chartName`                                                                                        | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Chart name (e.g., "acme-operator")                                                                 |
| `description`                                                                                      | *string*                                                                                           | :heavy_minus_sign:                                                                                 | Human-friendly description of the chart                                                            |
| `logCollector`                                                                                     | [models.PackagesConfigPatchLogCollector](../models/packagesconfigpatchlogcollector.md)             | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `enabled`                                                                                          | *boolean*                                                                                          | :heavy_minus_sign:                                                                                 | Whether Helm chart package generation is enabled                                                   |
| `setupResources`                                                                                   | [models.PackagesConfigPatchSetupResources](../models/packagesconfigpatchsetupresources.md)         | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `runtimePersistence`                                                                               | [models.PackagesConfigPatchRuntimePersistence](../models/packagesconfigpatchruntimepersistence.md) | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
