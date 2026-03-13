# DataPreparingEnvironment

## Example Usage

```typescript
import { DataPreparingEnvironment } from "@aliendotdev/platform-api/models";

let value: DataPreparingEnvironment = {
  strategyName: "<value>",
  type: "PreparingEnvironment",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `strategyName`                             | *string*                                   | :heavy_check_mark:                         | Name of the deployment strategy being used |
| `type`                                     | *"PreparingEnvironment"*                   | :heavy_check_mark:                         | N/A                                        |