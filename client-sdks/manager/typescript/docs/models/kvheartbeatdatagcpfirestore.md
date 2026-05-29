# KvHeartbeatDataGcpFirestore

## Example Usage

```typescript
import { KvHeartbeatDataGcpFirestore } from "@alienplatform/manager-api/models";

let value: KvHeartbeatDataGcpFirestore = {
  cmekEnabled: true,
  databaseName: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  sourceInfoPresent: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "gcpFirestore",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `appEngineIntegrationMode`                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `cmekEnabled`                                              | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `concurrencyMode`                                          | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `createTime`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `databaseEdition`                                          | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `databaseName`                                             | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `databaseType`                                             | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `deleteProtectionState`                                    | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `deleteTime`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `earliestVersionTime`                                      | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `endpoint`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `events`                                                   | [models.HeartbeatEvent](../models/heartbeatevent.md)[]     | :heavy_check_mark:                                         | N/A                                                        |
| `locationId`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `pointInTimeRecoveryEnablement`                            | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `projectId`                                                | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `sourceInfoPresent`                                        | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `status`                                                   | [models.KvHeartbeatStatus](../models/kvheartbeatstatus.md) | :heavy_check_mark:                                         | N/A                                                        |
| `updateTime`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `versionRetentionPeriod`                                   | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `backend`                                                  | *"gcpFirestore"*                                           | :heavy_check_mark:                                         | N/A                                                        |