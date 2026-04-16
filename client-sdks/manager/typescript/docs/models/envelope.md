# Envelope

Commands envelope sent to deployments

## Example Usage

```typescript
import { Envelope } from "@alienplatform/manager-api/models";

let value: Envelope = {
  attempt: 172468,
  command: "<value>",
  commandId: "<id>",
  deploymentId: "<id>",
  params: {
    inlineBase64: "<value>",
    mode: "inline",
  },
  protocol: "<value>",
  responseHandling: {
    maxInlineBytes: 579515,
    storageUploadRequest: {
      backend: {
        filePath: "/private/var/anenst.mar",
        operation: "delete",
        type: "local",
      },
      expiration: new Date("2026-06-22T00:08:29.133Z"),
      operation: "delete",
      path: "/usr",
    },
    submitResponseUrl: "https://unused-outlaw.net/",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `attempt`                                                                                     | *number*                                                                                      | :heavy_check_mark:                                                                            | Attempt number (starts at 1)                                                                  |
| `command`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | Command name (e.g., "generate-report", "sync-data")                                           |
| `commandId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique command identifier                                                                     |
| `deadline`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Command deadline                                                                              |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Target deployment identifier                                                                  |
| `params`                                                                                      | *models.BodySpec*                                                                             | :heavy_check_mark:                                                                            | Body specification supporting inline and storage modes                                        |
| `protocol`                                                                                    | *string*                                                                                      | :heavy_check_mark:                                                                            | Protocol version identifier                                                                   |
| `responseHandling`                                                                            | [models.ResponseHandling](../models/responsehandling.md)                                      | :heavy_check_mark:                                                                            | Response handling configuration for deployments                                               |