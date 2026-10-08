# RequiredPermissionAwStack

## Example Usage

```typescript
import { RequiredPermissionAwStack } from "@alienplatform/platform-api/models";

let value: RequiredPermissionAwStack = {
  resources: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `resources`                              | *string*[]                               | :heavy_check_mark:                       | N/A                                      |
| `condition`                              | Record<string, Record<string, *string*>> | :heavy_minus_sign:                       | N/A                                      |