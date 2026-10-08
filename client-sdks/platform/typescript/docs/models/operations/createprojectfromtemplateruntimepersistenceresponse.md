# CreateProjectFromTemplateRuntimePersistenceResponse

## Example Usage

```typescript
import { CreateProjectFromTemplateRuntimePersistenceResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateRuntimePersistenceResponse = {
  enabled: false,
  existingClaim: "<value>",
  size: "<value>",
  storageClassName: "<value>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `enabled`                                  | *boolean*                                  | :heavy_check_mark:                         | Persist operator identity across restarts. |
| `existingClaim`                            | *string*                                   | :heavy_check_mark:                         | N/A                                        |
| `size`                                     | *string*                                   | :heavy_check_mark:                         | N/A                                        |
| `storageClassName`                         | *string*                                   | :heavy_check_mark:                         | N/A                                        |
