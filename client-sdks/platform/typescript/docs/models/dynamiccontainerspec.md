# DynamicContainerSpec

## Example Usage

```typescript
import { DynamicContainerSpec } from "@alienplatform/platform-api/models";

let value: DynamicContainerSpec = {
  image: "https://loremflickr.com/82/3458?lock=4094663080609668",
  resources: {
    cpu: "<value>",
    memory: "<value>",
  },
  replicas: 184206,
  ports: [
    182584,
    415093,
  ],
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `image`                                                                            | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `resources`                                                                        | [models.DynamicContainerSpecResources](../models/dynamiccontainerspecresources.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `replicas`                                                                         | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `ports`                                                                            | *number*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `env`                                                                              | Record<string, *string*>                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `healthCheck`                                                                      | [models.HealthCheck](../models/healthcheck.md)                                     | :heavy_minus_sign:                                                                 | N/A                                                                                |