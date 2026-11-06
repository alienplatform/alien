# ActiveRelease

## Example Usage

```typescript
import { ActiveRelease } from "@alienplatform/platform-api/models";

let value: ActiveRelease = {
  id: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  version: "<value>",
  stack: {},
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              | Example                                                                                  |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `id`                                                                                     | *string*                                                                                 | :heavy_check_mark:                                                                       | Unique identifier for the release.                                                       | rel_WbhQgksrawSKIpEN0NAssHX9E2Su                                                         |
| `version`                                                                                | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |
| `stack`                                                                                  | [models.DeploymentLinkSetupResponseStack](../models/deploymentlinksetupresponsestack.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |                                                                                          |