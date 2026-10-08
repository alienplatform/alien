# CreateManagerResponseValidatedReleaseSelection

Immutable Release whose schema validated first-party inputs for this platform and channel.

## Example Usage

```typescript
import { CreateManagerResponseValidatedReleaseSelection } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseValidatedReleaseSelection = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  releaseChannel: "<value>",
  platform: "test",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        | Example                                                                            |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `releaseId`                                                                        | *string*                                                                           | :heavy_check_mark:                                                                 | Unique identifier for the release.                                                 | rel_WbhQgksrawSKIpEN0NAssHX9                                                       |
| `releaseChannel`                                                                   | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |                                                                                    |
| `platform`                                                                         | [models.CreateManagerResponsePlatform](../models/createmanagerresponseplatform.md) | :heavy_check_mark:                                                                 | Represents the target cloud platform.                                              |                                                                                    |