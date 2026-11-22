# DynamicContainerResponse

## Example Usage

```typescript
import { DynamicContainerResponse } from "@alienplatform/platform-api/models";

let value: DynamicContainerResponse = {
  name: "<value>",
  spec: {
    image: "https://picsum.photos/seed/K6bON0hss/1982/2719",
    resources: {
      cpu: "<value>",
      memory: "<value>",
    },
    replicas: 980768,
    ports: [
      729535,
      982360,
      9326,
    ],
  },
  generation: 65754,
  appliedGeneration: 277491,
  status: "failing",
  statusMessage: "<value>",
  internalServices: [
    {
      internalDns: "<value>",
      port: 650823,
    },
  ],
  deleted: false,
  createdAt: new Date("2026-06-26T04:38:55.653Z"),
  updatedAt: new Date("2026-08-07T07:13:45.726Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `spec`                                                                                        | [models.DynamicContainerSpec](../models/dynamiccontainerspec.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `generation`                                                                                  | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `appliedGeneration`                                                                           | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.DynamicContainerResponseStatus](../models/dynamiccontainerresponsestatus.md)          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `statusMessage`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `internalServices`                                                                            | [models.InternalService](../models/internalservice.md)[]                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deleted`                                                                                     | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |