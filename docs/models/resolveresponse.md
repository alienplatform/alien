# ResolveResponse

## Example Usage

```typescript
import { ResolveResponse } from "@alienplatform/platform-api/models";

let value: ResolveResponse = {
  managerId: "<id>",
  managerName: "<value>",
  managerUrl: "https://needy-papa.biz",
  managerIsSystem: true,
  projectId: "<id>",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `managerId`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Manager ID                                                                                               |
| `managerName`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Manager display name                                                                                     |
| `managerUrl`                                                                                             | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Manager URL                                                                                              |
| `managerIsSystem`                                                                                        | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | Whether the manager is Alien-hosted                                                                      |
| `managerCloud`                                                                                           | [models.ManagerCloud](../models/managercloud.md)                                                         | :heavy_minus_sign:                                                                                       | Cloud where the private manager is hosted. Null for Alien-hosted managers.                               |
| `projectId`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Resolved project ID                                                                                      |
| `installContext`                                                                                         | [models.ResolveResponseInstallContext](../models/resolveresponseinstallcontext.md)                       | :heavy_minus_sign:                                                                                       | Target install context derived from platform-managed manager metadata. Present for cloud push platforms. |