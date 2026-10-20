# DeploymentUpdateState

Durable progress for deployment updates

## Example Usage

```typescript
import { DeploymentUpdateState } from "@alienplatform/platform-api/models";

let value: DeploymentUpdateState = {
  active: null,
  next: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
    status: "applying",
    reasons: [],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    changedKeys: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    cause: {
      kind: "volume-restore",
      at: new Date("2024-04-20T16:58:36.345Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    },
    requestedAt: new Date("2025-07-21T11:59:25.062Z"),
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