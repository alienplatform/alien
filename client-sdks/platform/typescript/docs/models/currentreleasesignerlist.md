# CurrentReleaseSignerList

A statement as it is stored and carried.

## Example Usage

```typescript
import { CurrentReleaseSignerList } from "@alienplatform/platform-api/models";

let value: CurrentReleaseSignerList = {
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

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `payload`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | Base64 of the payload bytes.                                                                 |
| `signatures`                                                                                 | [models.CurrentReleaseSignerListSignature](../models/currentreleasesignerlistsignature.md)[] | :heavy_check_mark:                                                                           | Signatures over the payload bytes.                                                           |