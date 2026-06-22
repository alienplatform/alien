# CurrentReleaseOverrideStateGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseOverrideStateGcpResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseOverrideStateGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `condition`                                                | *models.CurrentReleaseOverrideStateResourceConditionUnion* | :heavy_minus_sign:                                         | N/A                                                        |
| `scope`                                                    | *string*                                                   | :heavy_check_mark:                                         | Scope (project/resource level)                             |