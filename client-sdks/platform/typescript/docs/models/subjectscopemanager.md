# SubjectScopeManager

## Example Usage

```typescript
import { SubjectScopeManager } from "@aliendotdev/platform-api/models";

let value: SubjectScopeManager = {
  type: "manager",
  managerId: "<id>",
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `type`                                           | *"manager"*                                      | :heavy_check_mark:                               | Manager-scoped access                            |
| `managerId`                                      | *string*                                         | :heavy_check_mark:                               | ID of the specific manager this scope applies to |