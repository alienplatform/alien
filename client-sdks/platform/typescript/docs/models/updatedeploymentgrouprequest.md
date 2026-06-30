# UpdateDeploymentGroupRequest

## Example Usage

```typescript
import { UpdateDeploymentGroupRequest } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentGroupRequest = {
  name: "prod-us-east-1",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            | Example                                                |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `name`                                                 | *string*                                               | :heavy_minus_sign:                                     | Deployment group name.                                 | prod-us-east-1                                         |
| `maxDeployments`                                       | *number*                                               | :heavy_minus_sign:                                     | Maximum number of deployments in this deployment group |                                                        |