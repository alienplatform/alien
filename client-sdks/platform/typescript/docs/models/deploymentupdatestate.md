# DeploymentUpdateState

Durable progress for deployment updates

## Example Usage

```typescript
import { DeploymentUpdateState } from "@alienplatform/platform-api/models";

let value: DeploymentUpdateState = {
  active: null,
  next: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
    status: "superseded",
    reasons: [
      "release",
    ],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    changedKeys: [
      "<value 1>",
    ],
    cause: null,
    requestedAt: new Date("2025-07-14T02:26:55.487Z"),
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