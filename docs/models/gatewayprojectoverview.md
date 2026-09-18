# GatewayProjectOverview

## Example Usage

```typescript
import { GatewayProjectOverview } from "@alienplatform/platform-api/models";

let value: GatewayProjectOverview = {
  id: "<id>",
  name: "<value>",
  state: "setup-in-progress",
  connectedCustomers: 778,
  settingUpCustomers: 213636,
  needsAttentionCustomers: 207371,
  revokedCustomers: 773029,
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `id`                                                           | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `name`                                                         | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `state`                                                        | [models.GatewayProjectState](../models/gatewayprojectstate.md) | :heavy_check_mark:                                             | N/A                                                            |
| `connectedCustomers`                                           | *number*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `settingUpCustomers`                                           | *number*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `needsAttentionCustomers`                                      | *number*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `revokedCustomers`                                             | *number*                                                       | :heavy_check_mark:                                             | N/A                                                            |