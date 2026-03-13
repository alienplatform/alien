# DataPushingStack

## Example Usage

```typescript
import { DataPushingStack } from "@aliendotdev/platform-api/models";

let value: DataPushingStack = {
  platform: "<value>",
  stack: "<value>",
  type: "PushingStack",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `platform`                     | *string*                       | :heavy_check_mark:             | Target platform                |
| `stack`                        | *string*                       | :heavy_check_mark:             | Name of the stack being pushed |
| `type`                         | *"PushingStack"*               | :heavy_check_mark:             | N/A                            |