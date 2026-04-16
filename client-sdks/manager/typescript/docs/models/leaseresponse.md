# LeaseResponse

Response to lease acquisition

## Example Usage

```typescript
import { LeaseResponse } from "@alienplatform/manager-api/models";

let value: LeaseResponse = {
  leases: [
    {
      attempt: 761121,
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
      leaseExpiresAt: new Date("2025-11-09T23:31:58.425Z"),
      leaseId: "<id>",
    },
  ],
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `leases`                                        | [models.LeaseInfo](../models/leaseinfo.md)[]    | :heavy_check_mark:                              | Acquired leases (empty array if none available) |