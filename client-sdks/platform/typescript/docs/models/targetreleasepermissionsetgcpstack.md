# TargetReleasePermissionSetGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleasePermissionSetGcpStack } from "@alienplatform/platform-api/models";

let value: TargetReleasePermissionSetGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `condition`                                          | *models.TargetReleasePermissionSetGcpConditionUnion* | :heavy_minus_sign:                                   | N/A                                                  |
| `scope`                                              | *string*                                             | :heavy_check_mark:                                   | Scope (project/resource level)                       |