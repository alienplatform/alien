# LeaseRequest

Request for acquiring leases

## Example Usage

```typescript
import { LeaseRequest } from "@alienplatform/manager-api/models";

let value: LeaseRequest = {
  deploymentId: "<id>",
};
```

## Fields

| Field                               | Type                                | Required                            | Description                         |
| ----------------------------------- | ----------------------------------- | ----------------------------------- | ----------------------------------- |
| `deploymentId`                      | *string*                            | :heavy_check_mark:                  | Deployment identifier               |
| `leaseSeconds`                      | *number*                            | :heavy_minus_sign:                  | Lease duration in seconds           |
| `maxLeases`                         | *number*                            | :heavy_minus_sign:                  | Maximum number of leases to acquire |