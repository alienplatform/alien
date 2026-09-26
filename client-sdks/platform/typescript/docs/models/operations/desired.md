# Desired

The release the deployment should run: its pinned release or its channel's release

## Example Usage

```typescript
import { Desired } from "@alienplatform/platform-api/models/operations";

let value: Desired = {
  releaseId: "<id>",
  version: "<value>",
  source: "channel",
  helmChart: {
    chart: "<value>",
    version: "<value>",
  },
  images: [
    "<value 1>",
    "<value 2>",
  ],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `releaseId`                                                          | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `version`                                                            | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `source`                                                             | [operations.DesiredSource](../../models/operations/desiredsource.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `helmChart`                                                          | [operations.HelmChart](../../models/operations/helmchart.md)         | :heavy_check_mark:                                                   | The Helm chart built for this release, when the release has one      |
| `images`                                                             | *string*[]                                                           | :heavy_check_mark:                                                   | Container images the release records for AWS, in resource order      |