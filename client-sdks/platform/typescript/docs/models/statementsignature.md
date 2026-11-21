# StatementSignature

## Example Usage

```typescript
import { StatementSignature } from "@alienplatform/platform-api/models";

let value: StatementSignature = {
  signer: "<value>",
  sig: "<value>",
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `signer`                                                | *string*                                                | :heavy_check_mark:                                      | Id of the signer on the signer list                     |
| `sig`                                                   | *string*                                                | :heavy_check_mark:                                      | Base64 DER ECDSA P-256 signature over the payload bytes |