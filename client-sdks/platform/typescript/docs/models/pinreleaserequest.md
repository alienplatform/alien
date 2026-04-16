# PinReleaseRequest

Request schema for pinning/unpinning agent release

## Example Usage

```typescript
import { PinReleaseRequest } from "@alienplatform/platform-api/models";

let value: PinReleaseRequest = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  | Example                                                                      |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `releaseId`                                                                  | *string*                                                                     | :heavy_minus_sign:                                                           | Release ID to pin the agent to. Set to null to unpin and use active release. | rel_WbhQgksrawSKIpEN0NAssHX9                                                 |