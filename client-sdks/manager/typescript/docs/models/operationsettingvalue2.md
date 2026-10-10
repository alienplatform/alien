# OperationSettingValue2

Stack resources the plugin may act on, such as buckets or queues.

## Example Usage

```typescript
import { OperationSettingValue2 } from "@alienplatform/manager-api/models";

let value: OperationSettingValue2 = {
  resources: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                           | Type                            | Required                        | Description                     |
| ------------------------------- | ------------------------------- | ------------------------------- | ------------------------------- |
| `resources`                     | *string*[]                      | :heavy_check_mark:              | Resource ids in the same stack. |
