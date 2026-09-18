# CurrentReleaseExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseExtendGcpResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `condition`                                         | *models.CurrentReleaseExtendResourceConditionUnion* | :heavy_minus_sign:                                  | N/A                                                 |
| `scope`                                             | *string*                                            | :heavy_check_mark:                                  | Scope (project/resource level)                      |