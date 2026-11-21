# SlackVendorAppBranding

## Example Usage

```typescript
import { SlackVendorAppBranding } from "@alienplatform/platform-api/models";

let value: SlackVendorAppBranding = {
  name: "<value>",
  description:
    "bookend birdcage youthfully gah gastropod sometimes considering",
  color: "violet",
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `name`                                           | *string*                                         | :heavy_check_mark:                               | The app's name, which customers see.             |
| `description`                                    | *string*                                         | :heavy_check_mark:                               | A short description, shown on the app's profile. |
| `color`                                          | *string*                                         | :heavy_check_mark:                               | Background color, `#RRGGBB`.                     |