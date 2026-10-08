# ScaleAutoscale

## Example Usage

```typescript
import { ScaleAutoscale } from "@alienplatform/platform-api/models";

let value: ScaleAutoscale = {
  type: "autoscale",
  min: {
    min: 243735,
    max: 780745,
    default: 799327,
  },
  max: {
    min: 504244,
    max: 889408,
    default: 145999,
  },
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `type`                         | *"autoscale"*                  | :heavy_check_mark:             | N/A                            |
| `min`                          | [models.Min](../models/min.md) | :heavy_check_mark:             | N/A                            |
| `max`                          | [models.Max](../models/max.md) | :heavy_check_mark:             | N/A                            |