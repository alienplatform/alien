# PutDynamicContainerRequest

## Example Usage

```typescript
import { PutDynamicContainerRequest } from "@alienplatform/platform-api/models";

let value: PutDynamicContainerRequest = {
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
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `spec`                                                           | [models.DynamicContainerSpec](../models/dynamiccontainerspec.md) | :heavy_check_mark:                                               | N/A                                                              |
| `secretEnv`                                                      | Record<string, *string*>                                         | :heavy_minus_sign:                                               | N/A                                                              |