# LastDeletedDeployment

## Example Usage

```typescript
import { LastDeletedDeployment } from "@alienplatform/platform-api/models";

let value: LastDeletedDeployment = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
  name: "<value>",
  completedAt: new Date("2024-06-28T21:11:59.520Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the deployment.                                                         | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                              |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `completedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |