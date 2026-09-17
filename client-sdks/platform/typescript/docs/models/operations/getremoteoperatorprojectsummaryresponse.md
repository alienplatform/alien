# GetRemoteOperatorProjectSummaryResponse

Authoritative Remote Operator state composed from existing owners.

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryResponse } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryResponse = {
  generatedAt: new Date("2024-04-05T10:31:21.050Z"),
  project: {
    id: "<id>",
    name: "<value>",
  },
  nextAction: {
    kind: "wait",
    target: "access",
    deploymentId: null,
    message: "<value>",
  },
  installations: {
    status: "ready",
    freshness: "fresh",
    sourceUpdatedAt: new Date("2025-05-24T23:27:55.042Z"),
    error: {
      code: "<value>",
      message: "<value>",
    },
    data: {
      items: [],
    },
  },
  release: {
    status: "error",
    freshness: "fresh",
    sourceUpdatedAt: new Date("2026-07-09T09:43:35.200Z"),
    error: {
      code: "<value>",
      message: "<value>",
    },
    data: {
      active: {
        id: "<id>",
        version: "<value>",
        createdAt: new Date("2026-02-12T11:27:57.156Z"),
      },
      rollout: {
        updated: 179604,
        updating: 403863,
        failed: 504443,
        pending: 107393,
        pinnedOther: 637480,
        superseded: 84741,
        onOther: 236888,
      },
    },
  },
  access: {
    status: "unavailable",
    freshness: "unknown",
    sourceUpdatedAt: null,
    error: {
      code: "<value>",
      message: "<value>",
    },
    data: {
      pending: 288049,
      active: 604277,
      recent: [],
    },
  },
  operations: {
    status: "ready",
    freshness: "unknown",
    sourceUpdatedAt: null,
    error: {
      code: "<value>",
      message: "<value>",
    },
    data: {
      selected: [],
      sync: {
        preparing: 460444,
        syncing: 482425,
        ready: 229664,
        stuck: 429061,
      },
      generatedPermissions: [],
    },
  },
  activity: {
    status: "ready",
    freshness: "stale",
    sourceUpdatedAt: new Date("2025-02-15T18:34:32.158Z"),
    error: {
      code: "<value>",
      message: "<value>",
    },
    data: {
      recent: [],
      lastVerifiedOperation: {
        invocationId: "<id>",
        deploymentId: "<id>",
        commandId: "<id>",
        plugin: "<value>",
        pluginVersion: "<value>",
        operation: "<value>",
        tier: "read-only",
        commandState: "SUCCEEDED",
        verificationState: "pending",
        createdAt: new Date("2025-10-24T19:06:22.305Z"),
        updatedAt: new Date("2024-01-09T03:03:29.645Z"),
        verification: {
          state: "skipped",
          attempts: 455281,
          maxAttempts: 907051,
          deadline: new Date("2025-08-07T07:04:22.954Z"),
          reason: "<value>",
        },
        sensitiveOutput: {
          kind: "redact",
          fields: [
            "<value 1>",
          ],
        },
        resultAvailable: true,
        completedAt: null,
      },
    },
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `generatedAt`                                                                                                          | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                          | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `project`                                                                                                              | [operations.GetRemoteOperatorProjectSummaryProject](../../models/operations/getremoteoperatorprojectsummaryproject.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `nextAction`                                                                                                           | [operations.NextAction](../../models/operations/nextaction.md)                                                         | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `installations`                                                                                                        | [operations.Installations](../../models/operations/installations.md)                                                   | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `release`                                                                                                              | [operations.Release](../../models/operations/release.md)                                                               | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `access`                                                                                                               | [operations.GetRemoteOperatorProjectSummaryAccess](../../models/operations/getremoteoperatorprojectsummaryaccess.md)   | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `operations`                                                                                                           | [operations.Operations](../../models/operations/operations.md)                                                         | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `activity`                                                                                                             | [operations.Activity](../../models/operations/activity.md)                                                             | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |