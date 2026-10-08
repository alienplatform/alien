# ManagerRetryResponseItem

## Example Usage

```typescript
import { ManagerRetryResponseItem } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseItem = {
  item: "models",
  source: {
    type: "project-release",
    releaseChannel: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  },
  required: false,
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `item`                                                                                     | [models.ManagerRetryResponseItemEnum](../models/managerretryresponseitemenum.md)           | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `source`                                                                                   | *models.ManagerRetryResponseSourceUnion*                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `required`                                                                                 | *boolean*                                                                                  | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `configuration`                                                                            | [models.ManagerRetryResponseConfiguration](../models/managerretryresponseconfiguration.md) | :heavy_minus_sign:                                                                         | N/A                                                                                        |