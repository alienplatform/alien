# GetDeploymentGroupRequest

## Example Usage

```typescript
import { GetDeploymentGroupRequest } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentGroupRequest = {
  id: "dg_r27ict8c7vcgsumpj90ackf7bnwt",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    | Example                                                                                        |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | Unique identifier for the deployment group.                                                    | dg_r27ict8c7vcgsumpj90ackf7bnwt                                                                |
| `include`                                                                                      | [operations.GetDeploymentGroupInclude](../../models/operations/getdeploymentgroupinclude.md)[] | :heavy_minus_sign:                                                                             | Optional fields to include: project                                                            |                                                                                                |