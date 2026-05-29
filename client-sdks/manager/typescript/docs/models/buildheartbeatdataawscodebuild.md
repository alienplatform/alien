# BuildHeartbeatDataAwsCodeBuild

## Example Usage

```typescript
import { BuildHeartbeatDataAwsCodeBuild } from "@alienplatform/manager-api/models";

let value: BuildHeartbeatDataAwsCodeBuild = {
  encryptionKeyPresent: false,
  environmentVariableCount: 168577,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  projectName: "<value>",
  serviceRolePresent: true,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "awsCodeBuild",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `artifactsEncryptionDisabled`                                    | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `artifactsType`                                                  | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `cloudWatchLogsStatus`                                           | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `computeType`                                                    | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `created`                                                        | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `description`                                                    | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `encryptionKeyPresent`                                           | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `environmentImage`                                               | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `environmentType`                                                | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `environmentVariableCount`                                       | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `events`                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]           | :heavy_check_mark:                                               | N/A                                                              |
| `imagePullCredentialsType`                                       | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `lastModified`                                                   | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `privilegedMode`                                                 | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `projectArn`                                                     | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `projectName`                                                    | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `queuedTimeoutInMinutes`                                         | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `s3LogsStatus`                                                   | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `serviceRolePresent`                                             | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `sourceType`                                                     | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [models.BuildHeartbeatStatus](../models/buildheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `timeoutInMinutes`                                               | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `backend`                                                        | *"awsCodeBuild"*                                                 | :heavy_check_mark:                                               | N/A                                                              |