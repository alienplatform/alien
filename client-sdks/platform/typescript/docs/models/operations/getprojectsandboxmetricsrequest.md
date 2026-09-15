# GetProjectSandboxMetricsRequest

## Example Usage

```typescript
import { GetProjectSandboxMetricsRequest } from "@alienplatform/platform-api/models/operations";

let value: GetProjectSandboxMetricsRequest = {
  idOrName: "<value>",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                                               | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Project ID or name.                                                                                                      |
| `range`                                                                                                                  | [operations.GetProjectSandboxMetricsQueryParamRange](../../models/operations/getprojectsandboxmetricsqueryparamrange.md) | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |