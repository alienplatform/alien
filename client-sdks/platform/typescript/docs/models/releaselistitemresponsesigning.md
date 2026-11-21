# ReleaseListItemResponseSigning

Who signed the release and for which channel; null when unsigned

## Example Usage

```typescript
import { ReleaseListItemResponseSigning } from "@alienplatform/platform-api/models";

let value: ReleaseListItemResponseSigning = {
  channel: "<value>",
  signers: [
    {
      id: "<id>",
      label: "<value>",
      kind: "ci",
    },
  ],
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `channel`                                                                            | *string*                                                                             | :heavy_check_mark:                                                                   | Channel the release was signed for                                                   |
| `signers`                                                                            | [models.ReleaseListItemResponseSigner](../models/releaselistitemresponsesigner.md)[] | :heavy_check_mark:                                                                   | Who signed it                                                                        |