# PersistImportedDeploymentRequestPreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `condition`                                                                      | *models.PersistImportedDeploymentRequestPreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                               | N/A                                                                              |
| `scope`                                                                          | *string*                                                                         | :heavy_check_mark:                                                               | Scope (project/resource level)                                                   |
