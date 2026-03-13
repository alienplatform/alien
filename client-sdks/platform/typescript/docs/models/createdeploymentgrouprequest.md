# CreateDeploymentGroupRequest

## Example Usage

```typescript
import { CreateDeploymentGroupRequest } from "@alienplatform/platform-api/models";

let value: CreateDeploymentGroupRequest = {
  name: "<value>",
  project: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `name`                                                 | *string*                                               | :heavy_check_mark:                                     | Name of the deployment group                           |
| `project`                                              | *string*                                               | :heavy_check_mark:                                     | Project ID or name this deployment group belongs to    |
| `maxDeployments`                                       | *number*                                               | :heavy_minus_sign:                                     | Maximum number of deployments in this deployment group |