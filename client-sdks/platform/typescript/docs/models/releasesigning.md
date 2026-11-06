# ReleaseSigning

Who signed the release and for which channel; null when unsigned

## Example Usage

```typescript
import { ReleaseSigning } from "@alienplatform/platform-api/models";

let value: ReleaseSigning = {
  channel: "<value>",
  signers: [],
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `channel`                                            | *string*                                             | :heavy_check_mark:                                   | Channel the release was signed for                   |
| `signers`                                            | [models.ReleaseSigner](../models/releasesigner.md)[] | :heavy_check_mark:                                   | Who signed it                                        |