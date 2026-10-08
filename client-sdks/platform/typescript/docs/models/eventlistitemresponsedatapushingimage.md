# EventListItemResponseDataPushingImage

## Example Usage

```typescript
import { EventListItemResponseDataPushingImage } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataPushingImage = {
  image: "https://picsum.photos/seed/3kNN0/505/3308",
  type: "PushingImage",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `image`                                     | *string*                                    | :heavy_check_mark:                          | Name of the image being pushed              |
| `progress`                                  | *models.EventListItemResponseProgressUnion* | :heavy_minus_sign:                          | N/A                                         |
| `type`                                      | *"PushingImage"*                            | :heavy_check_mark:                          | N/A                                         |
