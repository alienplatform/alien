# ResolveResponse

## Example Usage

```typescript
import { ResolveResponse } from "@alienplatform/platform-api/models";

let value: ResolveResponse = {
  managerId: "<id>",
  managerUrl: "https://needy-papa.biz",
  projectId: "<id>",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `managerId`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Manager ID                                                                                               |
| `managerUrl`                                                                                             | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Manager URL                                                                                              |
| `projectId`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Resolved project ID                                                                                      |
| `installContext`                                                                                         | [models.ResolveResponseInstallContext](../models/resolveresponseinstallcontext.md)                       | :heavy_minus_sign:                                                                                       | Target install context derived from platform-managed manager metadata. Present for cloud push platforms. |