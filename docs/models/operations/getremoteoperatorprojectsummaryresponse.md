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
    kind: "update",
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
      reported: [],
      older: 970663,
      unknown: 894335,
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
      investigations: [
        {
          id: "<id>",
          triggerType: "<value>",
          deploymentId: "<id>",
          status: "<value>",
          createdAt: new Date("2024-01-26T05:34:16.843Z"),
          updatedAt: new Date("2025-03-21T14:32:52.591Z"),
        },
      ],
      debugSessions: [
        {
          id: "<id>",
          deploymentId: "<id>",
          state: "New York",
          createdAt: new Date("2025-03-15T05:28:17.115Z"),
          expiresAt: new Date("2024-11-27T02:27:54.423Z"),
        },
      ],
      lastVerifiedOperation: {
        invocationId: "<id>",
        deploymentId: "<id>",
        commandId: "<id>",
        plugin: "<value>",
        pluginVersion: "<value>",
        operation: "<value>",
        tier: null,
        commandState: "SUCCEEDED",
        verificationState: "verified",
        createdAt: new Date("2024-12-22T11:58:06.410Z"),
        updatedAt: new Date("2024-12-20T02:15:50.612Z"),
        verification: {
          state: "skipped",
          attempts: 371867,
          maxAttempts: 533116,
          deadline: new Date("2024-06-09T09:14:41.952Z"),
          reason: "<value>",
        },
        sensitiveOutput: {
          kind: "none",
        },
        resultAvailable: false,
        completedAt: new Date("2025-12-16T19:53:47.814Z"),
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