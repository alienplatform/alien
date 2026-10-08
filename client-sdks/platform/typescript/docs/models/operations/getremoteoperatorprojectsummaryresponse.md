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
      unregisteredSetups: [
        {
          deploymentGroupId: "<id>",
          name: "<value>",
          platform: "kubernetes",
          createdAt: new Date("2024-03-02T14:19:50.314Z"),
          valuesExpireAt: new Date("2024-04-11T22:30:19.364Z"),
        },
      ],
    },
  },
  release: {
    status: "ready",
    freshness: "unknown",
    sourceUpdatedAt: new Date("2026-08-24T09:44:42.664Z"),
    error: {
      code: "<value>",
      message: "<value>",
    },
    data: {
      reported: [],
      older: 970663,
      unknown: 894335,
      rollout: [
        {
          releaseId: "<id>",
          version: "<value>",
          upToDate: 786693,
          waiting: 96641,
          unknown: 669432,
          failed: 401240,
        },
      ],
    },
  },
  access: {
    status: "unavailable",
    freshness: "fresh",
    sourceUpdatedAt: new Date("2024-09-24T15:28:43.771Z"),
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
    freshness: "fresh",
    sourceUpdatedAt: new Date("2024-05-17T16:26:04.347Z"),
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
    status: "empty",
    freshness: "stale",
    sourceUpdatedAt: new Date("2024-03-25T22:05:37.381Z"),
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
        accessRequestId: "<id>",
        commandState: "SUCCEEDED",
        verificationState: "pending",
        createdAt: new Date("2024-12-20T02:15:50.612Z"),
        updatedAt: new Date("2024-03-28T12:17:18.067Z"),
        verification: {
          state: "verified",
          attempts: 533116,
          maxAttempts: 249676,
          deadline: new Date("2025-03-26T23:32:49.615Z"),
          reason: "<value>",
        },
        sensitiveOutput: {
          kind: "requireConfirmation",
        },
        resultAvailable: true,
        completedAt: new Date("2025-05-08T17:12:42.798Z"),
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
