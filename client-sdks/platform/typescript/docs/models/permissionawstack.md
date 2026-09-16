# PermissionAwStack

## Example Usage

```typescript
import { PermissionAwStack } from "@alienplatform/platform-api/models";

let value: PermissionAwStack = {
  resources: [
    "<value 1>",
    "<value 2>",
  ],
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `resources`                              | *string*[]                               | :heavy_check_mark:                       | N/A                                      |
| `condition`                              | Record<string, Record<string, *string*>> | :heavy_minus_sign:                       | N/A                                      |