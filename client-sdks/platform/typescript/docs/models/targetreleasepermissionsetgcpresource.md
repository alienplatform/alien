# TargetReleasePermissionSetGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleasePermissionSetGcpResource } from "@alienplatform/platform-api/models";

let value: TargetReleasePermissionSetGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `condition`                                                  | *models.TargetReleasePermissionSetGcpResourceConditionUnion* | :heavy_minus_sign:                                           | N/A                                                          |
| `scope`                                                      | *string*                                                     | :heavy_check_mark:                                           | Scope (project/resource level)                               |