# GetManagerManagementConfigRequest

## Example Usage

```typescript
import { GetManagerManagementConfigRequest } from "@alienplatform/platform-api/models/operations";

let value: GetManagerManagementConfigRequest = {
  id: "mgr_enxscjrqiiu2lrc672hwwuc5",
  platform: "machines",
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        | Example                                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                               | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Unique identifier for a manager.                                                                                                   | mgr_enxscjrqiiu2lrc672hwwuc5                                                                                                       |
| `platform`                                                                                                                         | [operations.GetManagerManagementConfigQueryParamPlatform](../../models/operations/getmanagermanagementconfigqueryparamplatform.md) | :heavy_check_mark:                                                                                                                 | Represents the target cloud platform.                                                                                              |                                                                                                                                    |