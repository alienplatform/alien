# SubjectScopeProject

## Example Usage

```typescript
import { SubjectScopeProject } from "@alienplatform/platform-api/models";

let value: SubjectScopeProject = {
  type: "project",
  projectId: "<id>",
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `type`                                           | *"project"*                                      | :heavy_check_mark:                               | Project-scoped access                            |
| `projectId`                                      | *string*                                         | :heavy_check_mark:                               | ID of the specific project this scope applies to |