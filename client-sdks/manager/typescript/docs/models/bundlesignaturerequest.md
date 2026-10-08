# BundleSignatureRequest

Body of `POST /v1/deployments/{id}/bundle-signature`.

## Example Usage

```typescript
import { BundleSignatureRequest } from "@alienplatform/manager-api/models";

let value: BundleSignatureRequest = {
  manifest: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `manifest`                                                         | *string*                                                           | :heavy_check_mark:                                                 | The bundle's `manifest.json`, base64-encoded, exactly as packaged. |