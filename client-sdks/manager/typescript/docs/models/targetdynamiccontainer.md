# TargetDynamicContainer

One release-independent container the Operator should run in its namespace.
The manager sends the complete set on every sync. An empty set removes
containers previously owned by this deployment.

## Example Usage

```typescript
import { TargetDynamicContainer } from "@alienplatform/manager-api/models";

let value: TargetDynamicContainer = {
  cpu: "<value>",
  deleted: true,
  generation: 1560,
  image: "https://loremflickr.com/408/3724?lock=7958450983752649",
  memory: "<value>",
  name: "<value>",
  ports: [
    771504,
    81418,
    65719,
  ],
  replicas: 271152,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `cpu`                                                                          | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `deleted`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `env`                                                                          | Record<string, *string*>                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `generation`                                                                   | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `healthCheck`                                                                  | [models.DynamicContainerHealthCheck](../models/dynamiccontainerhealthcheck.md) | :heavy_minus_sign:                                                             | N/A                                                                            |
| `image`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `ports`                                                                        | *number*[]                                                                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `replicas`                                                                     | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `secretEnv`                                                                    | Record<string, *string*>                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `suspendedReason`                                                              | *string*                                                                       | :heavy_minus_sign:                                                             | Stop an installed workload when its release no longer admits the image.        |