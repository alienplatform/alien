# EventDataPreparingEnvironment

## Example Usage

```typescript
import { EventDataPreparingEnvironment } from "@alienplatform/platform-api/models";

let value: EventDataPreparingEnvironment = {
  strategyName: "<value>",
  type: "PreparingEnvironment",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `strategyName`                             | *string*                                   | :heavy_check_mark:                         | Name of the deployment strategy being used |
| `type`                                     | *"PreparingEnvironment"*                   | :heavy_check_mark:                         | N/A                                        |
