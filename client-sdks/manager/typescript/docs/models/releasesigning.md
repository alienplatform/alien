# ReleaseSigning

A signed release's channel and signers, for the release badge. Display
only: decoded, never verified, because the manager isn't trusted to.

## Example Usage

```typescript
import { ReleaseSigning } from "@alienplatform/manager-api/models";

let value: ReleaseSigning = {
  channel: "<value>",
  signers: [],
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `channel`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | Channel the statements sign, such as `production`.                                           |
| `signers`                                                                                    | [models.ReleaseSigner](../models/releasesigner.md)[]                                         | :heavy_check_mark:                                                                           | Signers of the statements, with their label and kind from the signer<br/>list version they name. |
