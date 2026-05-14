# DeploymentInfoHelm

## Example Usage

```typescript
import { DeploymentInfoHelm } from "@alienplatform/platform-api/models";

let value: DeploymentInfoHelm = {
  status: "pending",
  chartRef: "<value>",
  managerFetchExample: "<value>",
  localImportExample: "<value>",
};
```

## Fields

| Field                                          | Type                                           | Required                                       | Description                                    |
| ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- |
| `status`                                       | [models.HelmStatus](../models/helmstatus.md)   | :heavy_check_mark:                             | Status of a package build                      |
| `version`                                      | *string*                                       | :heavy_minus_sign:                             | N/A                                            |
| `outputs`                                      | [models.HelmOutputs](../models/helmoutputs.md) | :heavy_minus_sign:                             | Outputs from a Helm chart package build        |
| `error`                                        | *any*                                          | :heavy_minus_sign:                             | N/A                                            |
| `chartRef`                                     | *string*                                       | :heavy_check_mark:                             | OCI chart reference                            |
| `managerFetchExample`                          | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `localImportExample`                           | *string*                                       | :heavy_check_mark:                             | N/A                                            |