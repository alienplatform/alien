# DataPushingImage

## Example Usage

```typescript
import { DataPushingImage } from "@aliendotdev/platform-api/models";

let value: DataPushingImage = {
  image: "https://picsum.photos/seed/bgd6b4HoNE/948/3236",
  type: "PushingImage",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `image`                        | *string*                       | :heavy_check_mark:             | Name of the image being pushed |
| `progress`                     | *any*                          | :heavy_minus_sign:             | N/A                            |
| `type`                         | *"PushingImage"*               | :heavy_check_mark:             | N/A                            |