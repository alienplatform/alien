# PromoteReleaseRequestBody

## Example Usage

```typescript
import { PromoteReleaseRequestBody } from "@alienplatform/platform-api/models/operations";

let value: PromoteReleaseRequestBody = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  expectedReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        | Example                            |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `releaseId`                        | *string*                           | :heavy_check_mark:                 | Unique identifier for the release. | rel_WbhQgksrawSKIpEN0NAssHX9       |
| `expectedReleaseId`                | *string*                           | :heavy_minus_sign:                 | Unique identifier for the release. | rel_WbhQgksrawSKIpEN0NAssHX9       |