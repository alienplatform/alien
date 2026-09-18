# ImportedResource

## Example Usage

```typescript
import { ImportedResource } from "@alienplatform/platform-api/models";

let value: ImportedResource = {
  id: "<id>",
  type: "<value>",
  importData: {
    "key": "<value>",
  },
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                       | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource id from the active stack                                                                                                                          |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |
| `importData`                                                                                                                                               | Record<string, *any*>                                                                                                                                      | :heavy_check_mark:                                                                                                                                         | Resolved typed import payload                                                                                                                              |