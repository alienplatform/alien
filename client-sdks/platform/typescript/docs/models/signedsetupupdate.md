# SignedSetupUpdate

Protected deployments: the setup it waits for is the newer signed setup of this release (GET /deployments/{id}/signed-setup-update has its steps), not a rerun of the installed one.

## Example Usage

```typescript
import { SignedSetupUpdate } from "@alienplatform/platform-api/models";

let value: SignedSetupUpdate = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  templateDigest: "<value>",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        | Example                            |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `releaseId`                        | *string*                           | :heavy_check_mark:                 | Unique identifier for the release. | rel_WbhQgksrawSKIpEN0NAssHX9E2Su   |
| `templateDigest`                   | *string*                           | :heavy_check_mark:                 | N/A                                |                                    |