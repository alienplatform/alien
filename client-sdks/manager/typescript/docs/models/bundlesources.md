# BundleSources

The artifacts a bundle carries besides the release's own images.

## Example Usage

```typescript
import { BundleSources } from "@alienplatform/manager-api/models";

let value: BundleSources = {
  chart: {
    credentials: "caller",
    reference: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `chart`                                                    | [models.BundleSource](../models/bundlesource.md)           | :heavy_check_mark:                                         | One OCI artifact and how to authenticate when pulling it.  |
| `deployCli`                                                | [models.ToolDownload](../models/tooldownload.md)[]         | :heavy_minus_sign:                                         | `alien-deploy` builds for the site, matching this manager. |