# EventListItemResponseDataPushingStack

## Example Usage

```typescript
import { EventListItemResponseDataPushingStack } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataPushingStack = {
  platform: "<value>",
  stack: "<value>",
  type: "PushingStack",
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `destination`                                | *string*                                     | :heavy_minus_sign:                           | Human-readable destination for pushed images |
| `platform`                                   | *string*                                     | :heavy_check_mark:                           | Target platform                              |
| `stack`                                      | *string*                                     | :heavy_check_mark:                           | Name of the stack being pushed               |
| `type`                                       | *"PushingStack"*                             | :heavy_check_mark:                           | N/A                                          |
