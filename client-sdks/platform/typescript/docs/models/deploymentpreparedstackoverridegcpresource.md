# DeploymentPreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { DeploymentPreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `condition`                                                    | *models.DeploymentPreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                             | N/A                                                            |
| `scope`                                                        | *string*                                                       | :heavy_check_mark:                                             | Scope (project/resource level)                                 |
