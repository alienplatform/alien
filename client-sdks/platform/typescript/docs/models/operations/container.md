# Container

## Example Usage

```typescript
import { Container } from "@alienplatform/platform-api/models/operations";

let value: Container = {
  name: "<value>",
  image: "https://picsum.photos/seed/LShtZ8/229/2392",
  status: "pending",
  currentReplicas: 209434,
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `name`                                                                                                 | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `image`                                                                                                | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [operations.ListDeploymentContainersStatus](../../models/operations/listdeploymentcontainersstatus.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `currentReplicas`                                                                                      | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |