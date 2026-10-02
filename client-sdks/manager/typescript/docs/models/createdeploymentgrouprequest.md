# CreateDeploymentGroupRequest

## Example Usage

```typescript
import { CreateDeploymentGroupRequest } from "@alienplatform/manager-api/models";

let value: CreateDeploymentGroupRequest = {
  name: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `environmentVariables`                                              | [models.EnvironmentVariable](../models/environmentvariable.md)[]    | :heavy_minus_sign:                                                  | Environment variables applied to each deployment the group creates. |
| `inputValues`                                                       | [models.InputValues](../models/inputvalues.md)                      | :heavy_minus_sign:                                                  | Stack input values applied to each deployment the group creates.    |
| `maxDeployments`                                                    | *number*                                                            | :heavy_minus_sign:                                                  | N/A                                                                 |
| `name`                                                              | *string*                                                            | :heavy_check_mark:                                                  | N/A                                                                 |