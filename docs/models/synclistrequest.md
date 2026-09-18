# SyncListRequest

Request to list full operational deployments

## Example Usage

```typescript
import { SyncListRequest } from "@alienplatform/platform-api/models";

let value: SyncListRequest = {
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  deploymentIds: [
    "dep_0c29fq4a2yjb7kx3smwdgxlc",
  ],
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              | Example                                                                  |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `managerId`                                                              | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      | mgr_enxscjrqiiu2lrc672hwwuc5                                             |
| `deploymentIds`                                                          | *string*[]                                                               | :heavy_minus_sign:                                                       | Specific deployment IDs to include                                       |                                                                          |
| `statuses`                                                               | [models.SyncListRequestStatus](../models/synclistrequeststatus.md)[]     | :heavy_minus_sign:                                                       | Filter by deployment status                                              |                                                                          |
| `platforms`                                                              | [models.SyncListRequestPlatform](../models/synclistrequestplatform.md)[] | :heavy_minus_sign:                                                       | Filter by deployment platform                                            |                                                                          |
| `deploymentGroupId`                                                      | *string*                                                                 | :heavy_minus_sign:                                                       | Filter by deployment group ID                                            | dg_r27ict8c7vcgsumpj90ackf7b                                             |
| `limit`                                                                  | *number*                                                                 | :heavy_minus_sign:                                                       | Maximum records to return                                                |                                                                          |