# DeploymentDetailResponseUpdateState

Durable progress for deployment updates

## Example Usage

```typescript
import { DeploymentDetailResponseUpdateState } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseUpdateState = {
  active: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
    status: "queued",
    reasons: [],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    changedKeys: [],
    cause: {
      kind: "volume-restore",
      at: new Date("2024-04-20T16:58:36.345Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    },
    requestedAt: new Date("2024-05-03T02:14:14.983Z"),
  },
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
  latest: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
    status: "queued",
    reasons: [],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    changedKeys: [
      "<value 1>",
      "<value 2>",
    ],
    cause: {
      kind: "volume-restore",
      at: new Date("2024-04-20T16:58:36.345Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    },
    requestedAt: new Date("2026-06-26T04:52:21.740Z"),
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `active`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `next`                                                                                   | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `latest`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |