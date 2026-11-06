# CurrentReleaseSignedSignature

One signature over a statement's payload.

## Example Usage

```typescript
import { CurrentReleaseSignedSignature } from "@alienplatform/platform-api/models";

let value: CurrentReleaseSignedSignature = {
  sig: "<value>",
  signer: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `sig`                                                    | *string*                                                 | :heavy_check_mark:                                       | Base64 DER ECDSA P-256 signature over the payload bytes. |
| `signer`                                                 | *string*                                                 | :heavy_check_mark:                                       | Id of the signer on the signer list.                     |