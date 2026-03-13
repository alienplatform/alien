# DataPushingResource

## Example Usage

```typescript
import { DataPushingResource } from "@aliendotdev/platform-api/models";

let value: DataPushingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "PushingResource",
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `resourceName`                                          | *string*                                                | :heavy_check_mark:                                      | Name of the resource being pushed                       |
| `resourceType`                                          | *string*                                                | :heavy_check_mark:                                      | Type of the resource: "function", "container", "worker" |
| `type`                                                  | *"PushingResource"*                                     | :heavy_check_mark:                                      | N/A                                                     |