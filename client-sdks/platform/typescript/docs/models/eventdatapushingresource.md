# EventDataPushingResource

## Example Usage

```typescript
import { EventDataPushingResource } from "@alienplatform/platform-api/models";

let value: EventDataPushingResource = {
  resourceName: "<value>",
  resourceType: "<value>",
  type: "PushingResource",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `resourceName`                              | *string*                                    | :heavy_check_mark:                          | Name of the resource being pushed           |
| `resourceType`                              | *string*                                    | :heavy_check_mark:                          | Type of the resource: "worker", "container" |
| `type`                                      | *"PushingResource"*                         | :heavy_check_mark:                          | N/A                                         |
