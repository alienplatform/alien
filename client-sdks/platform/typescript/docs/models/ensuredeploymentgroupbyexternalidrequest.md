# EnsureDeploymentGroupByExternalIdRequest

## Example Usage

```typescript
import { EnsureDeploymentGroupByExternalIdRequest } from "@alienplatform/platform-api/models";

let value: EnsureDeploymentGroupByExternalIdRequest = {
  externalId: "ext_example_01",
  name: "prod-us-east-1",
  project: "<value>",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 | Example                                                     |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `externalId`                                                | *string*                                                    | :heavy_check_mark:                                          | Case-sensitive identifier from the integrating application. | ext_example_01                                              |
| `name`                                                      | *string*                                                    | :heavy_check_mark:                                          | Deployment group name.                                      | prod-us-east-1                                              |
| `project`                                                   | *string*                                                    | :heavy_check_mark:                                          | Project ID or name this deployment group belongs to         |                                                             |
| `maxDeployments`                                            | *number*                                                    | :heavy_minus_sign:                                          | Maximum number of deployments for newly created groups      |                                                             |