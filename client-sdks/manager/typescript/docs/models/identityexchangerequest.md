# IdentityExchangeRequest

## Example Usage

```typescript
import { IdentityExchangeRequest } from "@alienplatform/manager-api/models";

let value: IdentityExchangeRequest = {
  deploymentId: "<id>",
  proof: {
    token: "<value>",
    type: "session",
  },
  purpose: {
    type: "operatorOperations",
  },
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `deploymentId`                                                          | *string*                                                                | :heavy_check_mark:                                                      | Selects the binding to verify; never establishes membership by itself.  |
| `proof`                                                                 | *models.RuntimeIdentityProof*                                           | :heavy_check_mark:                                                      | Proof material is intentionally not Debug: providers must never log it. |
| `purpose`                                                               | *models.RuntimePurpose*                                                 | :heavy_check_mark:                                                      | Runtime operations cannot administer deployments or mint credentials.   |
