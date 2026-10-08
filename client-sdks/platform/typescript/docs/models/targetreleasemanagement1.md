# TargetReleaseManagement1

## Example Usage

```typescript
import { TargetReleaseManagement1 } from "@alienplatform/platform-api/models";

let value: TargetReleaseManagement1 = {
  extend: {
    "key": [
      {
        description: "highly experienced lavish revitalise against flu solder",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [],
    "key2": [
      {
        description: "highly experienced lavish revitalise against flu solder",
        id: "<id>",
        platforms: {},
      },
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.TargetReleaseExtendUnion*[]>                                                                               | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |