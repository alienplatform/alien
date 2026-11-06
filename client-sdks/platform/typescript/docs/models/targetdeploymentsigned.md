# TargetDeploymentSigned

A statement as it is stored and carried.

## Example Usage

```typescript
import { TargetDeploymentSigned } from "@alienplatform/platform-api/models";

let value: TargetDeploymentSigned = {
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

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `payload`                                                                                | *string*                                                                                 | :heavy_check_mark:                                                                       | Base64 of the payload bytes.                                                             |
| `signatures`                                                                             | [models.TargetDeploymentSignedSignature](../models/targetdeploymentsignedsignature.md)[] | :heavy_check_mark:                                                                       | Signatures over the payload bytes.                                                       |