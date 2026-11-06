# GenerateManagerBindingTokenRequest

## Example Usage

```typescript
import { GenerateManagerBindingTokenRequest } from "@alienplatform/platform-api/models";

let value: GenerateManagerBindingTokenRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
};
```

## Fields

| Field                                                                                                                     | Type                                                                                                                      | Required                                                                                                                  | Description                                                                                                               | Example                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                            | *string*                                                                                                                  | :heavy_check_mark:                                                                                                        | Unique identifier for the deployment.                                                                                     | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f                                                                                          |
| `kind`                                                                                                                    | [models.GenerateManagerBindingTokenRequestKind](../models/generatemanagerbindingtokenrequestkind.md)                      | :heavy_minus_sign:                                                                                                        | Binding kind the token resolves. Defaults to the deployment's own kind: sandbox for a sandbox deployment, data otherwise. |                                                                                                                           |