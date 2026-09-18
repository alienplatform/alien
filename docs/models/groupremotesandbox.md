# GroupRemoteSandbox

## Example Usage

```typescript
import { GroupRemoteSandbox } from "@alienplatform/platform-api/models";

let value: GroupRemoteSandbox = {
  capability: "keys",
  enabled: true,
  state: "not-connected",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  observation: {
    provider: "<value>",
    platform: "gcp",
    resourceId: "<id>",
    location: "<value>",
    account: null,
    observedAt: new Date("2025-11-23T13:41:17.370Z"),
    stale: true,
    partial: true,
    health: "healthy",
    lifecycle: "<value>",
    message: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    | Example                                                                        |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `capability`                                                                   | [models.RemoteSandboxCapability](../models/remotesandboxcapability.md)         | :heavy_check_mark:                                                             | N/A                                                                            |                                                                                |
| `enabled`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |                                                                                |
| `state`                                                                        | [models.RemoteSandboxState](../models/remotesandboxstate.md)                   | :heavy_check_mark:                                                             | N/A                                                                            |                                                                                |
| `deploymentId`                                                                 | *string*                                                                       | :heavy_check_mark:                                                             | Unique identifier for the deployment.                                          | dep_0c29fq4a2yjb7kx3smwdgxlc                                                   |
| `observation`                                                                  | [models.RemoteSandboxObservation](../models/remotesandboxobservation.md)       | :heavy_check_mark:                                                             | N/A                                                                            |                                                                                |
| `modelCoverage`                                                                | [models.RemoteSandboxModelCoverage](../models/remotesandboxmodelcoverage.md)[] | :heavy_minus_sign:                                                             | N/A                                                                            |                                                                                |
| `directProvider`                                                               | [models.RemoteSandboxDirectProvider](../models/remotesandboxdirectprovider.md) | :heavy_minus_sign:                                                             | N/A                                                                            |                                                                                |
| `root`                                                                         | [models.RemoteSandboxRoot](../models/remotesandboxroot.md)                     | :heavy_minus_sign:                                                             | N/A                                                                            |                                                                                |
| `registry`                                                                     | [models.RemoteSandboxRegistry](../models/remotesandboxregistry.md)             | :heavy_minus_sign:                                                             | N/A                                                                            |                                                                                |