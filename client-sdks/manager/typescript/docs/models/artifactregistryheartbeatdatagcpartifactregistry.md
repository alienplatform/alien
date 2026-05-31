# ArtifactRegistryHeartbeatDataGcpArtifactRegistry

## Example Usage

```typescript
import { ArtifactRegistryHeartbeatDataGcpArtifactRegistry } from "@alienplatform/manager-api/models";

let value: ArtifactRegistryHeartbeatDataGcpArtifactRegistry = {
  cleanupPolicyCount: 419391,
  iamBindingCount: 128533,
  iamPolicyEtagPresent: false,
  iamRoles: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  kmsKeyNamePresent: true,
  labelCount: 439989,
  location: "<value>",
  projectId: "<id>",
  repositoryId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "gcpArtifactRegistry",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `cleanupPolicyCount`                                                                   | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `cleanupPolicyDryRun`                                                                  | *boolean*                                                                              | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `createTime`                                                                           | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `description`                                                                          | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `format`                                                                               | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `iamBindingCount`                                                                      | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `iamPolicyEtagPresent`                                                                 | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `iamRoles`                                                                             | *string*[]                                                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `kmsKeyNamePresent`                                                                    | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `labelCount`                                                                           | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `location`                                                                             | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `mode`                                                                                 | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `name`                                                                                 | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `projectId`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `pullServiceAccountEmail`                                                              | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `pushServiceAccountEmail`                                                              | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `repositoryId`                                                                         | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `satisfiesPzs`                                                                         | *boolean*                                                                              | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `sizeBytes`                                                                            | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `status`                                                                               | [models.ArtifactRegistryHeartbeatStatus](../models/artifactregistryheartbeatstatus.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `updateTime`                                                                           | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `backend`                                                                              | *"gcpArtifactRegistry"*                                                                | :heavy_check_mark:                                                                     | N/A                                                                                    |