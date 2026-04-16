# LeaseInfo

Lease information

## Example Usage

```typescript
import { LeaseInfo } from "@alienplatform/manager-api/models";

let value: LeaseInfo = {
  attempt: 444775,
  commandId: "<id>",
  envelope: {
    attempt: 536354,
    command: "<value>",
    commandId: "<id>",
    deploymentId: "<id>",
    params: {
      mode: "storage",
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
  },
  leaseExpiresAt: new Date("2025-06-23T02:24:27.150Z"),
  leaseId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `attempt`                                                                                     | *number*                                                                                      | :heavy_check_mark:                                                                            | Attempt number                                                                                |
| `commandId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Command identifier                                                                            |
| `envelope`                                                                                    | [models.Envelope](../models/envelope.md)                                                      | :heavy_check_mark:                                                                            | Commands envelope sent to deployments                                                         |
| `leaseExpiresAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When lease expires                                                                            |
| `leaseId`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique lease identifier                                                                       |