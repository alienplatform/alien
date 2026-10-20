# Cause

What asked for this update, when it was recorded

## Example Usage

```typescript
import { Cause } from "@alienplatform/platform-api/models";

let value: Cause = {
  kind: "retry",
  at: new Date("2024-07-03T00:36:30.162Z"),
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        | Example                                                                                            |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `kind`                                                                                             | [models.CauseKind](../models/causekind.md)                                                         | :heavy_check_mark:                                                                                 | The action that asked for the update                                                               |                                                                                                    |
| `actor`                                                                                            | [models.DeploymentUpdateOperationSummaryActor](../models/deploymentupdateoperationsummaryactor.md) | :heavy_minus_sign:                                                                                 | Who took the action, when known                                                                    |                                                                                                    |
| `at`                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)      | :heavy_check_mark:                                                                                 | When the action was taken                                                                          |                                                                                                    |
| `channel`                                                                                          | *string*                                                                                           | :heavy_minus_sign:                                                                                 | The promoted channel, or the channel the deployment moved to                                       |                                                                                                    |
| `releaseId`                                                                                        | *string*                                                                                           | :heavy_minus_sign:                                                                                 | The promoted or pinned release                                                                     | rel_WbhQgksrawSKIpEN0NAssHX9                                                                       |