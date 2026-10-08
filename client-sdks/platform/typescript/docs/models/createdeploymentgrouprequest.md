# CreateDeploymentGroupRequest

## Example Usage

```typescript
import { CreateDeploymentGroupRequest } from "@alienplatform/platform-api/models";

let value: CreateDeploymentGroupRequest = {
  name: "prod-us-east-1",
  externalId: "ext_example_01",
  project: "<value>",
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       | Example                                                                           |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `name`                                                                            | *string*                                                                          | :heavy_check_mark:                                                                | Deployment group name.                                                            | prod-us-east-1                                                                    |
| `externalId`                                                                      | *string*                                                                          | :heavy_minus_sign:                                                                | Case-sensitive, URL- and header-safe identifier from the integrating application. | ext_example_01                                                                    |
| `project`                                                                         | *string*                                                                          | :heavy_check_mark:                                                                | Project ID or name this deployment group belongs to                               |                                                                                   |
| `maxDeployments`                                                                  | *number*                                                                          | :heavy_minus_sign:                                                                | Maximum number of deployments in this deployment group                            |                                                                                   |