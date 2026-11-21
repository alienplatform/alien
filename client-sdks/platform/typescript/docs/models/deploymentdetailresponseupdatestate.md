# DeploymentDetailResponseUpdateState

Durable progress for deployment updates

## Example Usage

```typescript
import { DeploymentDetailResponseUpdateState } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseUpdateState = {
  active: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
    status: "queued",
    reasons: [],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    changedKeys: [],
    blocker: {
      kind: "other",
      message: "<value>",
      signedSetupUpdate: {
        releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        templateDigest: "<value>",
      },
    },
    cause: {
      kind: "promotion",
      at: new Date("2026-12-01T10:04:11.187Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    },
    requestedAt: new Date("2025-05-23T02:17:08.731Z"),
  },
  next: {
    id: "duop_0vtxpb1sw4sbcdwg2xo37q6msm9l",
    status: "superseded",
    reasons: [
      "redeploy",
    ],
    targetReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    changedKeys: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    blocker: {
      kind: "other",
      message: "<value>",
      signedSetupUpdate: {
        releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        templateDigest: "<value>",
      },
    },
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
    blocker: {
      kind: "other",
      message: "<value>",
      signedSetupUpdate: {
        releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
        templateDigest: "<value>",
      },
    },
    cause: {
      kind: "promotion",
      at: new Date("2026-12-01T10:04:11.187Z"),
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    },
    requestedAt: new Date("2026-04-09T17:19:50.603Z"),
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `active`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `next`                                                                                   | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `latest`                                                                                 | [models.DeploymentUpdateOperationSummary](../models/deploymentupdateoperationsummary.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |