# BundleSignatureResponse

A signature over a bundle manifest.

## Example Usage

```typescript
import { BundleSignatureResponse } from "@alienplatform/manager-api/models";

let value: BundleSignatureResponse = {
  publicKey: "<value>",
  signature: "<value>",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `publicKey`                                           | *string*                                              | :heavy_check_mark:                                    | `ed25519:<base64>` public key that verifies it.       |
| `signature`                                           | *string*                                              | :heavy_check_mark:                                    | `ed25519:<base64>` signature over the manifest bytes. |