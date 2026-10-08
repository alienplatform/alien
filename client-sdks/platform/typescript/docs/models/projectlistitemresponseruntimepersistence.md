# ProjectListItemResponseRuntimePersistence

## Example Usage

```typescript
import { ProjectListItemResponseRuntimePersistence } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseRuntimePersistence = {
  enabled: true,
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