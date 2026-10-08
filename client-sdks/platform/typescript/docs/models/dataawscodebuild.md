# DataAwsCodeBuild

## Example Usage

```typescript
import { DataAwsCodeBuild } from "@alienplatform/platform-api/models";

let value: DataAwsCodeBuild = {
  encryptionKeyPresent: true,
  environmentVariableCount: 879452,
  projectName: "<value>",
  serviceRolePresent: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  backend: "awsCodeBuild",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `artifactsEncryptionDisabled`                                              | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `artifactsType`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `cloudWatchLogsStatus`                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `computeType`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `created`                                                                  | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `description`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `encryptionKeyPresent`                                                     | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `environmentImage`                                                         | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `environmentType`                                                          | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `environmentVariableCount`                                                 | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `imagePullCredentialsType`                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `lastModified`                                                             | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `privilegedMode`                                                           | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `projectArn`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `projectName`                                                              | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `queuedTimeoutInMinutes`                                                   | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `s3LogsStatus`                                                             | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `serviceRolePresent`                                                       | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `sourceType`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus56](../models/resourceheartbeatstatus56.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `timeoutInMinutes`                                                         | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `backend`                                                                  | *"awsCodeBuild"*                                                           | :heavy_check_mark:                                                         | N/A                                                                        |