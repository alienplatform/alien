# EventDataPushingImage

## Example Usage

```typescript
import { EventDataPushingImage } from "@alienplatform/platform-api/models";

let value: EventDataPushingImage = {
  image: "https://picsum.photos/seed/3ETRvL33XQ/3838/1575",
  type: "PushingImage",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `image`                        | *string*                       | :heavy_check_mark:             | Name of the image being pushed |
| `progress`                     | *models.EventProgressUnion*    | :heavy_minus_sign:             | N/A                            |
| `type`                         | *"PushingImage"*               | :heavy_check_mark:             | N/A                            |
