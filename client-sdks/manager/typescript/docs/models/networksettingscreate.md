# NetworkSettingsCreate

Create a new isolated VPC/VNet with a managed NAT gateway.

All networking infrastructure is provisioned by Alien and cleaned up on delete.
VMs use private IPs only; all outbound traffic routes through the NAT gateway.

Recommended for production deployments.

## Example Usage

```typescript
import { NetworkSettingsCreate } from "@alienplatform/manager-api/models";

let value: NetworkSettingsCreate = {
  type: "create",
};
```

## Fields

| Field                                                                                                               | Type                                                                                                                | Required                                                                                                            | Description                                                                                                         |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `availabilityZones`                                                                                                 | *number*                                                                                                            | :heavy_minus_sign:                                                                                                  | Number of availability zones (default: 2).                                                                          |
| `cidr`                                                                                                              | *string*                                                                                                            | :heavy_minus_sign:                                                                                                  | VPC/VNet CIDR block. If not specified, auto-generated from stack ID<br/>to reduce conflicts (e.g., "10.{hash}.0.0/16"). |
| `type`                                                                                                              | *"create"*                                                                                                          | :heavy_check_mark:                                                                                                  | N/A                                                                                                                 |