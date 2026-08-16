# GroupBuckets

## Example Usage

```typescript
import { GroupBuckets } from "@alienplatform/platform-api/models";

let value: GroupBuckets = {
  capability: "registry",
  enabled: false,
  state: "not-connected",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  observation: {
    provider: "<value>",
    platform: "test",
    resourceId: "<id>",
    location: "<value>",
    account: "58292455",
    observedAt: new Date("2024-09-29T01:47:42.152Z"),
    stale: false,
    partial: false,
    health: "unknown",
    lifecycle: "<value>",
    message: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        | Example                                                            |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `capability`                                                       | [models.BucketsCapability](../models/bucketscapability.md)         | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `enabled`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `state`                                                            | [models.BucketsState](../models/bucketsstate.md)                   | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `deploymentId`                                                     | *string*                                                           | :heavy_check_mark:                                                 | Unique identifier for the deployment.                              | dep_0c29fq4a2yjb7kx3smwdgxlc                                       |
| `observation`                                                      | [models.BucketsObservation](../models/bucketsobservation.md)       | :heavy_check_mark:                                                 | N/A                                                                |                                                                    |
| `modelCoverage`                                                    | [models.BucketsModelCoverage](../models/bucketsmodelcoverage.md)[] | :heavy_minus_sign:                                                 | N/A                                                                |                                                                    |
| `directProvider`                                                   | [models.BucketsDirectProvider](../models/bucketsdirectprovider.md) | :heavy_minus_sign:                                                 | N/A                                                                |                                                                    |
| `root`                                                             | [models.BucketsRoot](../models/bucketsroot.md)                     | :heavy_minus_sign:                                                 | N/A                                                                |                                                                    |
| `registry`                                                         | [models.BucketsRegistry](../models/bucketsregistry.md)             | :heavy_minus_sign:                                                 | N/A                                                                |                                                                    |