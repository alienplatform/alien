# GroupKeys

## Example Usage

```typescript
import { GroupKeys } from "@alienplatform/platform-api/models";

let value: GroupKeys = {
  capability: "models",
  enabled: true,
  state: "revoked",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  observation: {
    provider: "<value>",
    platform: "gcp",
    resourceId: "<id>",
    location: "<value>",
    account: "19573632",
    observedAt: new Date("2024-06-20T03:52:32.683Z"),
    stale: true,
    partial: true,
    health: "degraded",
    lifecycle: "<value>",
    message: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  | Example                                                      |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `capability`                                                 | [models.KeysCapability](../models/keyscapability.md)         | :heavy_check_mark:                                           | N/A                                                          |                                                              |
| `enabled`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |                                                              |
| `state`                                                      | [models.KeysState](../models/keysstate.md)                   | :heavy_check_mark:                                           | N/A                                                          |                                                              |
| `deploymentId`                                               | *string*                                                     | :heavy_check_mark:                                           | Unique identifier for the deployment.                        | dep_0c29fq4a2yjb7kx3smwdgxlc                                 |
| `observation`                                                | [models.KeysObservation](../models/keysobservation.md)       | :heavy_check_mark:                                           | N/A                                                          |                                                              |
| `modelCoverage`                                              | [models.KeysModelCoverage](../models/keysmodelcoverage.md)[] | :heavy_minus_sign:                                           | N/A                                                          |                                                              |
| `directProvider`                                             | [models.KeysDirectProvider](../models/keysdirectprovider.md) | :heavy_minus_sign:                                           | N/A                                                          |                                                              |
| `root`                                                       | [models.KeysRoot](../models/keysroot.md)                     | :heavy_minus_sign:                                           | N/A                                                          |                                                              |
| `registry`                                                   | [models.KeysRegistry](../models/keysregistry.md)             | :heavy_minus_sign:                                           | N/A                                                          |                                                              |