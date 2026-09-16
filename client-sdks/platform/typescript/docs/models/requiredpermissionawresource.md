# RequiredPermissionAwResource

## Example Usage

```typescript
import { RequiredPermissionAwResource } from "@alienplatform/platform-api/models";

let value: RequiredPermissionAwResource = {
  resources: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `resources`                              | *string*[]                               | :heavy_check_mark:                       | N/A                                      |
| `condition`                              | Record<string, Record<string, *string*>> | :heavy_minus_sign:                       | N/A                                      |