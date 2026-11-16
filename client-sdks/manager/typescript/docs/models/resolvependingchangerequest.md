# ResolvePendingChangeRequest

Body of `POST /v1/pending-changes/{id}/resolution`.

## Example Usage

```typescript
import { ResolvePendingChangeRequest } from "@alienplatform/manager-api/models";

let value: ResolvePendingChangeRequest = {
  status: "rejected",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `signerListVersion`                                                      | *number*                                                                 | :heavy_minus_sign:                                                       | The signer list version that carries the change; required when approved. |
| `status`                                                                 | [models.ResolutionStatus](../models/resolutionstatus.md)                 | :heavy_check_mark:                                                       | How a pending change ended.                                              |
