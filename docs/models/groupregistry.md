# GroupRegistry

## Example Usage

```typescript
import { GroupRegistry } from "@alienplatform/platform-api/models";

let value: GroupRegistry = {
  capability: "models",
  enabled: true,
  state: "revoked",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  observation: {
    provider: "<value>",
    platform: "gcp",
    resourceId: "<id>",
    location: "<value>",
    account: "07843950",
    observedAt: new Date("2026-07-02T12:17:56.583Z"),
    stale: true,
    partial: true,
    health: "unknown",
    lifecycle: null,
    message: "<value>",
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          | Example                                                              |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `capability`                                                         | [models.RegistryCapability](../models/registrycapability.md)         | :heavy_check_mark:                                                   | N/A                                                                  |                                                                      |
| `enabled`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |                                                                      |
| `state`                                                              | [models.RegistryState](../models/registrystate.md)                   | :heavy_check_mark:                                                   | N/A                                                                  |                                                                      |
| `deploymentId`                                                       | *string*                                                             | :heavy_check_mark:                                                   | Unique identifier for the deployment.                                | dep_0c29fq4a2yjb7kx3smwdgxlc                                         |
| `observation`                                                        | [models.RegistryObservation](../models/registryobservation.md)       | :heavy_check_mark:                                                   | N/A                                                                  |                                                                      |
| `modelCoverage`                                                      | [models.RegistryModelCoverage](../models/registrymodelcoverage.md)[] | :heavy_minus_sign:                                                   | N/A                                                                  |                                                                      |
| `directProvider`                                                     | [models.RegistryDirectProvider](../models/registrydirectprovider.md) | :heavy_minus_sign:                                                   | N/A                                                                  |                                                                      |
| `root`                                                               | [models.RegistryRoot](../models/registryroot.md)                     | :heavy_minus_sign:                                                   | N/A                                                                  |                                                                      |
| `registry`                                                           | [models.GroupRegistryRegistry](../models/groupregistryregistry.md)   | :heavy_minus_sign:                                                   | N/A                                                                  |                                                                      |