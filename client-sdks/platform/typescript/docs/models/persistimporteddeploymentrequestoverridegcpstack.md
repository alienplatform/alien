# PersistImportedDeploymentRequestOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `condition`                                                          | *models.PersistImportedDeploymentRequestOverrideStackConditionUnion* | :heavy_minus_sign:                                                   | N/A                                                                  |
| `scope`                                                              | *string*                                                             | :heavy_check_mark:                                                   | Scope (project/resource level)                                       |