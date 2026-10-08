# ManagerRetryResponseValidatedReleaseSelection

Immutable Release whose schema validated first-party inputs for this platform and channel.

## Example Usage

```typescript
import { ManagerRetryResponseValidatedReleaseSelection } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseValidatedReleaseSelection = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  releaseChannel: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      | Example                                                                          |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `releaseId`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | Unique identifier for the release.                                               | rel_WbhQgksrawSKIpEN0NAssHX9                                                     |
| `releaseChannel`                                                                 | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |                                                                                  |
| `platform`                                                                       | [models.ManagerRetryResponsePlatform](../models/managerretryresponseplatform.md) | :heavy_check_mark:                                                               | Represents the target cloud platform.                                            |                                                                                  |