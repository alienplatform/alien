# ConfigureProjectSourceStorage

## Example Usage

```typescript
import { ConfigureProjectSourceStorage } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceStorage = {
  enabled: true,
  access: "read-write",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                          | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `access`                                                                                           | [operations.ConfigureProjectSourceAccess](../../models/operations/configureprojectsourceaccess.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |