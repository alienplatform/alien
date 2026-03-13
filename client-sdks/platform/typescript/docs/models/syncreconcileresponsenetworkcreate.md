# SyncReconcileResponseNetworkCreate

## Example Usage

```typescript
import { SyncReconcileResponseNetworkCreate } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseNetworkCreate = {
  type: "create",
};
```

## Fields

| Field                                                                                                               | Type                                                                                                                | Required                                                                                                            | Description                                                                                                         |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `availabilityZones`                                                                                                 | *number*                                                                                                            | :heavy_minus_sign:                                                                                                  | Number of availability zones (default: 2).                                                                          |
| `cidr`                                                                                                              | *string*                                                                                                            | :heavy_minus_sign:                                                                                                  | VPC/VNet CIDR block. If not specified, auto-generated from stack ID<br/>to reduce conflicts (e.g., "10.{hash}.0.0/16"). |
| `type`                                                                                                              | [models.SyncReconcileResponseTypeCreate](../models/syncreconcileresponsetypecreate.md)                              | :heavy_check_mark:                                                                                                  | N/A                                                                                                                 |