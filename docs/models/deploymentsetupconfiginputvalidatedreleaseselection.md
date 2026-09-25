# DeploymentSetupConfigInputValidatedReleaseSelection

Immutable Release whose schema validated first-party inputs for this platform and channel.

## Example Usage

```typescript
import { DeploymentSetupConfigInputValidatedReleaseSelection } from "@alienplatform/platform-api/models";

let value: DeploymentSetupConfigInputValidatedReleaseSelection = {
  releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  releaseChannel: "<value>",
  platform: "kubernetes",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  | Example                                                                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `releaseId`                                                                                  | *string*                                                                                     | :heavy_check_mark:                                                                           | Unique identifier for the release.                                                           | rel_WbhQgksrawSKIpEN0NAssHX9                                                                 |
| `releaseChannel`                                                                             | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |                                                                                              |
| `platform`                                                                                   | [models.DeploymentSetupConfigInputPlatform](../models/deploymentsetupconfiginputplatform.md) | :heavy_check_mark:                                                                           | Represents the target cloud platform.                                                        |                                                                                              |