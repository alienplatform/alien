# TargetReleaseSignerList

A statement as it is stored and carried.

## Example Usage

```typescript
import { TargetReleaseSignerList } from "@alienplatform/platform-api/models";

let value: TargetReleaseSignerList = {
  payload: "<value>",
  signatures: [],
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `payload`                                                                                  | *string*                                                                                   | :heavy_check_mark:                                                                         | Base64 of the payload bytes.                                                               |
| `signatures`                                                                               | [models.TargetReleaseSignerListSignature](../models/targetreleasesignerlistsignature.md)[] | :heavy_check_mark:                                                                         | Signatures over the payload bytes.                                                         |