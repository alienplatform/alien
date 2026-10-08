# PersistImportedDeploymentRequestPreparedStackOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `condition`                                                                          | *models.PersistImportedDeploymentRequestPreparedStackOverrideResourceConditionUnion* | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `scope`                                                                              | *string*                                                                             | :heavy_check_mark:                                                                   | Scope (project/resource level)                                                       |
