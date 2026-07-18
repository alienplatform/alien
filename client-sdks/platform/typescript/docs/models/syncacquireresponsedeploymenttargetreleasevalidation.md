# SyncAcquireResponseDeploymentTargetReleaseValidation

Portable stack input validation constraints.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleaseValidation } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleaseValidation = {};
```

## Fields

| Field                               | Type                                | Required                            | Description                         |
| ----------------------------------- | ----------------------------------- | ----------------------------------- | ----------------------------------- |
| `format`                            | *string*                            | :heavy_minus_sign:                  | Semantic format hint such as url.   |
| `max`                               | *string*                            | :heavy_minus_sign:                  | Maximum number.                     |
| `maxItems`                          | *number*                            | :heavy_minus_sign:                  | Maximum string-list items.          |
| `maxLength`                         | *number*                            | :heavy_minus_sign:                  | Maximum string length.              |
| `min`                               | *string*                            | :heavy_minus_sign:                  | Minimum number.                     |
| `minItems`                          | *number*                            | :heavy_minus_sign:                  | Minimum string-list items.          |
| `minLength`                         | *number*                            | :heavy_minus_sign:                  | Minimum string length.              |
| `pattern`                           | *string*                            | :heavy_minus_sign:                  | Portable whole-value regex pattern. |
| `values`                            | *string*[]                          | :heavy_minus_sign:                  | Allowed string enum values.         |