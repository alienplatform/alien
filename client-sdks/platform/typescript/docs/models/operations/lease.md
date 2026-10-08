# Lease

## Example Usage

```typescript
import { Lease } from "@alienplatform/platform-api/models/operations";

let value: Lease = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  renewed: false,
  leaseExpiresAt: new Date("2025-02-04T06:29:01.550Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `renewed`                                                                                     | *boolean*                                                                                     | :heavy_check_mark:                                                                            | False when this session no longer holds a live lease.                                         |                                                                                               |
| `leaseExpiresAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
