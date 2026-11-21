# DeploymentGroupInfo

## Example Usage

```typescript
import { DeploymentGroupInfo } from "@alienplatform/platform-api/models";

let value: DeploymentGroupInfo = {
  id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
  name: "prod-us-east-1",
  externalId: "ext_example_01",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    | Example                                                                                        |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | Unique identifier for the deployment group.                                                    | dg_r27ict8c7vcgsumpj90ackf7bnwt                                                                |
| `name`                                                                                         | *string*                                                                                       | :heavy_check_mark:                                                                             | Deployment group name.                                                                         | prod-us-east-1                                                                                 |
| `externalId`                                                                                   | *string*                                                                                       | :heavy_check_mark:                                                                             | Case-sensitive, URL- and header-safe identifier from the integrating application.              | ext_example_01                                                                                 |
| `deploymentCount`                                                                              | *number*                                                                                       | :heavy_minus_sign:                                                                             | Total deployments visible to the caller in this customer, independent of pagination and search |                                                                                                |