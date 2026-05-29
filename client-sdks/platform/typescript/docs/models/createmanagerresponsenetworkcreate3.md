# CreateManagerResponseNetworkCreate3

## Example Usage

```typescript
import { CreateManagerResponseNetworkCreate3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseNetworkCreate3 = {
  type: "create",
};
```

## Fields

| Field                                                                                                               | Type                                                                                                                | Required                                                                                                            | Description                                                                                                         |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `availabilityZones`                                                                                                 | *number*                                                                                                            | :heavy_minus_sign:                                                                                                  | Number of availability zones (default: 2).                                                                          |
| `cidr`                                                                                                              | *string*                                                                                                            | :heavy_minus_sign:                                                                                                  | VPC/VNet CIDR block. If not specified, auto-generated from stack ID<br/>to reduce conflicts (e.g., "10.{hash}.0.0/16"). |
| `type`                                                                                                              | [models.SetupTypeCreate3](../models/setuptypecreate3.md)                                                            | :heavy_check_mark:                                                                                                  | N/A                                                                                                                 |