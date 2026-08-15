# DeploymentUpdateState

Durable progress for deployment updates

## Example Usage

```typescript
import { DeploymentUpdateState } from "@alienplatform/platform-api/models";

let value: DeploymentUpdateState = {
  active: null,
  next: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
    status: "queued",
    reasons: [],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    changedKeys: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    requestedAt: new Date("2025-05-23T02:17:08.731Z"),
  },
  latest: null,
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `active`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `next`                                                                                   | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `latest`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |