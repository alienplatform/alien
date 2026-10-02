# SignDeploymentBundleRequest

## Example Usage

```typescript
import { SignDeploymentBundleRequest } from "@alienplatform/manager-api/models/operations";

let value: SignDeploymentBundleRequest = {
  id: "<id>",
  bundleSignatureRequest: {
    manifest: "<value>",
  },
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `id`                                                                    | *string*                                                                | :heavy_check_mark:                                                      | Deployment ID                                                           |
| `bundleSignatureRequest`                                                | [models.BundleSignatureRequest](../../models/bundlesignaturerequest.md) | :heavy_check_mark:                                                      | N/A                                                                     |