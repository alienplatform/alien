# TargetReleaseApprovalPlugins

## Example Usage

```typescript
import { TargetReleaseApprovalPlugins } from "@alienplatform/platform-api/models";

let value: TargetReleaseApprovalPlugins = {
  decision: "auto",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `decision`                                                                                                           | [models.TargetReleasePluginsDecision](../models/targetreleasepluginsdecision.md)                                     | :heavy_check_mark:                                                                                                   | Whether matching operations run without approval.                                                                    |
| `maxRisk`                                                                                                            | *string*                                                                                                             | :heavy_minus_sign:                                                                                                   | Highest risk tier (`read-only`, `mutating`, `destructive`) a wildcard<br/>access request for these operations may cover. |