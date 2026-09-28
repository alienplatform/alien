# PutDynamicContainerRequest

## Example Usage

```typescript
import { PutDynamicContainerRequest } from "@alienplatform/platform-api/models/operations";

let value: PutDynamicContainerRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  name: "<value>",
  putDynamicContainerRequest: {
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
  },
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     | Example                                                                         |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `id`                                                                            | *string*                                                                        | :heavy_check_mark:                                                              | Unique identifier for the deployment.                                           | dep_0c29fq4a2yjb7kx3smwdgxlc                                                    |
| `name`                                                                          | *string*                                                                        | :heavy_check_mark:                                                              | N/A                                                                             |                                                                                 |
| `ifMatch`                                                                       | *string*                                                                        | :heavy_minus_sign:                                                              | N/A                                                                             |                                                                                 |
| `putDynamicContainerRequest`                                                    | [models.PutDynamicContainerRequest](../../models/putdynamiccontainerrequest.md) | :heavy_check_mark:                                                              | N/A                                                                             |                                                                                 |