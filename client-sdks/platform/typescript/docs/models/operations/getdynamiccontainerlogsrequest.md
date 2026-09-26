# GetDynamicContainerLogsRequest

## Example Usage

```typescript
import { GetDynamicContainerLogsRequest } from "@alienplatform/platform-api/models/operations";

let value: GetDynamicContainerLogsRequest = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  name: "<value>",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           | Example                               |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `id`                                  | *string*                              | :heavy_check_mark:                    | Unique identifier for the deployment. | dep_0c29fq4a2yjb7kx3smwdgxlc          |
| `name`                                | *string*                              | :heavy_check_mark:                    | N/A                                   |                                       |
| `sinceMinutes`                        | *number*                              | :heavy_minus_sign:                    | N/A                                   |                                       |
| `limit`                               | *number*                              | :heavy_minus_sign:                    | N/A                                   |                                       |
| `cursor`                              | *string*                              | :heavy_minus_sign:                    | N/A                                   |                                       |