# SyncRenewResponse

Renewal processed. Inspect `leases` for per-deployment outcomes; a lost lease is reported there rather than failing the whole batch.

## Example Usage

```typescript
import { SyncRenewResponse } from "@alienplatform/platform-api/models/operations";

let value: SyncRenewResponse = {
  success: false,
  leases: [
    {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      renewed: false,
      leaseExpiresAt: new Date("2025-11-17T01:11:25.058Z"),
    },
  ],
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `success`                                              | *boolean*                                              | :heavy_check_mark:                                     | True when every requested deployment was renewed.      |
| `leases`                                               | [operations.Lease](../../models/operations/lease.md)[] | :heavy_check_mark:                                     | Per-deployment renewal outcome.                        |
