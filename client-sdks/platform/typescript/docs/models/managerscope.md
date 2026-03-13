# ManagerScope

Manager-scoped configuration

## Example Usage

```typescript
import { ManagerScope } from "@aliendotdev/platform-api/models";

let value: ManagerScope = {
  type: "manager",
  managerId: "<id>",
  role: "manager.runtime",
};
```

## Fields

| Field                                          | Type                                           | Required                                       | Description                                    | Example                                        |
| ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- |
| `type`                                         | *"manager"*                                    | :heavy_check_mark:                             | N/A                                            |                                                |
| `managerId`                                    | *string*                                       | :heavy_check_mark:                             | ID of the manager this is scoped to            |                                                |
| `role`                                         | [models.ManagerRole](../models/managerrole.md) | :heavy_check_mark:                             | Role for manager-scoped service accounts       | workspace.member                               |