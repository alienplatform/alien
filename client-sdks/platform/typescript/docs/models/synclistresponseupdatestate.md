# SyncListResponseUpdateState

Durable progress for deployment updates

## Example Usage

```typescript
import { SyncListResponseUpdateState } from "@alienplatform/platform-api/models";

let value: SyncListResponseUpdateState = {
  active: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
    status: "applying",
    reasons: [],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    changedKeys: [],
    requestedAt: new Date("2026-04-28T10:12:17.277Z"),
  },
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
  latest: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6",
    status: "superseded",
    reasons: [
      "redeploy",
    ],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    changedKeys: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    requestedAt: new Date("2024-11-23T19:43:18.153Z"),
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `active`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `next`                                                                                   | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `latest`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |