# CurrentReleaseSigned

A statement as it is stored and carried.

## Example Usage

```typescript
import { CurrentReleaseSigned } from "@alienplatform/platform-api/models";

let value: CurrentReleaseSigned = {
  payload: "<value>",
  signatures: [
    {
      sig: "<value>",
      signer: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `payload`                                                                            | *string*                                                                             | :heavy_check_mark:                                                                   | Base64 of the payload bytes.                                                         |
| `signatures`                                                                         | [models.CurrentReleaseSignedSignature](../models/currentreleasesignedsignature.md)[] | :heavy_check_mark:                                                                   | Signatures over the payload bytes.                                                   |