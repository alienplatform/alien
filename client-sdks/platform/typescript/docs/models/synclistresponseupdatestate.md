# SyncListResponseUpdateState

Durable progress for deployment updates

## Example Usage

```typescript
import { SyncListResponseUpdateState } from "@alienplatform/platform-api/models";

let value: SyncListResponseUpdateState = {
  active: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
    status: "blocked",
    reasons: [
      "release",
    ],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    changedKeys: [],
    cause: {
      kind: "volume-restore",
      at: new Date("2025-05-23T02:17:08.731Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    },
    requestedAt: new Date("2025-05-26T07:07:30.861Z"),
  },
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
  latest: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
    status: "failed",
    reasons: [],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    changedKeys: [
      "<value 1>",
      "<value 2>",
    ],
    cause: {
      kind: "volume-restore",
      at: new Date("2025-05-23T02:17:08.731Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    },
    requestedAt: new Date("2026-10-10T15:20:06.831Z"),
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `active`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `next`                                                                                   | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `latest`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |