# DeploymentDetailResponseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentDetailResponseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `condition`                                                    | *models.DeploymentDetailResponseProfileResourceConditionUnion* | :heavy_minus_sign:                                             | N/A                                                            |
| `scope`                                                        | *string*                                                       | :heavy_check_mark:                                             | Scope (project/resource level)                                 |