# GroupModels

## Example Usage

```typescript
import { GroupModels } from "@alienplatform/platform-api/models";

let value: GroupModels = {
  capability: "keys",
  enabled: true,
  state: "revoked",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  observation: {
    provider: "<value>",
    platform: "local",
    resourceId: "<id>",
    location: "<value>",
    account: "22393779",
    observedAt: new Date("2025-02-05T09:16:14.437Z"),
    stale: false,
    partial: true,
    health: null,
    lifecycle: "<value>",
    message: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      | Example                                                          |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `capability`                                                     | [models.ModelsCapability](../models/modelscapability.md)         | :heavy_check_mark:                                               | N/A                                                              |                                                                  |
| `enabled`                                                        | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |                                                                  |
| `state`                                                          | [models.ModelsState](../models/modelsstate.md)                   | :heavy_check_mark:                                               | N/A                                                              |                                                                  |
| `deploymentId`                                                   | *string*                                                         | :heavy_check_mark:                                               | Unique identifier for the deployment.                            | dep_0c29fq4a2yjb7kx3smwdgxlc                                     |
| `observation`                                                    | [models.ModelsObservation](../models/modelsobservation.md)       | :heavy_check_mark:                                               | N/A                                                              |                                                                  |
| `modelCoverage`                                                  | [models.ModelsModelCoverage](../models/modelsmodelcoverage.md)[] | :heavy_minus_sign:                                               | N/A                                                              |                                                                  |
| `directProvider`                                                 | [models.ModelsDirectProvider](../models/modelsdirectprovider.md) | :heavy_minus_sign:                                               | N/A                                                              |                                                                  |
| `root`                                                           | [models.ModelsRoot](../models/modelsroot.md)                     | :heavy_minus_sign:                                               | N/A                                                              |                                                                  |
| `registry`                                                       | [models.ModelsRegistry](../models/modelsregistry.md)             | :heavy_minus_sign:                                               | N/A                                                              |                                                                  |